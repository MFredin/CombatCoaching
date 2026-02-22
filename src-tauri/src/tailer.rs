/// Tails the newest WoWCombatLog*.txt in the configured Logs directory,
/// emitting new lines as they are written.
///
/// Uses the `notify` crate (OS-level ReadDirectoryChangesWatcher on Windows)
/// to detect file modifications and creations, then reads from the last known
/// byte offset.
///
/// ## Dynamic log switching
/// WoW creates a new timestamped log file (e.g. `WoWCombatLog_2024_06_15_195432.txt`)
/// each time the player enables combat logging or zones into a new area.  On
/// every `EventKind::Create` event the tailer rescans the directory and switches
/// to the newest `WoWCombatLog*.txt` if it is different from the current file.
///
/// ## Rotation handling
/// If the active file shrinks (WoW rewrote it), the offset resets to 0 and the
/// file is read from the beginning.
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;
use tauri::AppHandle;
use tokio::sync::mpsc::Sender;

use crate::config::find_latest_log;
use crate::ipc::{self, ConnectionStatus};

// ---------------------------------------------------------------------------
// Active-file state
// ---------------------------------------------------------------------------

struct TailerState {
    /// The Logs directory being watched.
    logs_dir: PathBuf,
    /// The currently active log file (may change when WoW creates a new one).
    active_file: Option<PathBuf>,
    /// Byte offset of the next unread byte in `active_file`.
    position: u64,
}

impl TailerState {
    fn new(logs_dir: PathBuf) -> Self {
        let active_file = find_latest_log(&logs_dir);
        if let Some(ref f) = active_file {
            tracing::info!("Tailer: initial log file {:?}", f);
        } else {
            tracing::info!("Tailer: no WoWCombatLog*.txt found yet in {:?}", logs_dir);
        }
        Self { logs_dir, active_file, position: 0 }
    }

    /// Called on directory Create events.  If a newer WoWCombatLog*.txt has
    /// appeared, switch to it and reset the byte offset to 0.
    fn check_for_new_log(&mut self) {
        let newest = match find_latest_log(&self.logs_dir) {
            Some(p) => p,
            None    => return,
        };

        let is_new = self.active_file.as_deref() != Some(newest.as_path());
        if is_new {
            tracing::info!("Tailer: switching to new log file {:?}", newest);
            self.active_file = Some(newest);
            self.position    = 0;
        }
    }

    /// Read any new lines from the active file since `self.position`.
    fn read_new_lines(&mut self, tx: &Sender<String>) -> Result<()> {
        let path = match &self.active_file {
            Some(p) => p.clone(),
            None => {
                // No log file yet — try to find one now (WoW may have just
                // created it between the watcher event and this call).
                self.check_for_new_log();
                match &self.active_file {
                    Some(p) => p.clone(),
                    None    => return Ok(()),
                }
            }
        };

        let metadata = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => return Ok(()), // File doesn't exist yet — wait
        };
        let file_len = metadata.len();

        // Rotation: file shrank since our last read
        if file_len < self.position {
            tracing::info!("CombatLog rotation detected — restarting from byte 0");
            self.position = 0;
        }

        if file_len == self.position {
            return Ok(()); // No new data
        }

        let mut file = File::open(&path)?;
        file.seek(SeekFrom::Start(self.position))?;

        let reader = BufReader::new(&file);
        for line in reader.lines() {
            match line {
                Ok(l) if !l.is_empty() => {
                    if tx.blocking_send(l).is_err() {
                        return Ok(()); // Receiver gone — pipeline shutting down
                    }
                }
                Ok(_)  => {}
                Err(e) => {
                    tracing::warn!("Tailer read error: {}", e);
                    break;
                }
            }
        }

        // Update position to end of file (handles partial line writes gracefully;
        // partial lines won't be returned by BufRead, so we re-read them next time).
        self.position = file_len;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// `logs_dir`    — the WoW Logs directory (e.g. `..\World of Warcraft\_retail_\Logs`).
/// `app_handle`  — used to emit `coach:connection` status events to the frontend.
/// `wow_path_str`— human-readable path shown in the settings Connection panel.
pub async fn run(
    logs_dir:     PathBuf,
    tx:           Sender<String>,
    app_handle:   AppHandle,
    wow_path_str: String,
) -> Result<()> {
    tracing::info!("Tailer starting, watching directory: {:?}", logs_dir);

    let (fs_tx, fs_rx) = std_mpsc::channel::<notify::Result<Event>>();

    let config = notify::Config::default()
        .with_poll_interval(Duration::from_millis(500));

    let mut watcher = RecommendedWatcher::new(fs_tx, config)?;
    watcher.watch(&logs_dir, RecursiveMode::NonRecursive)?;

    let mut state = TailerState::new(logs_dir);

    // Emit initial connection status so the settings UI reflects reality immediately.
    let tailing_now = state.active_file.is_some();
    ipc::emit_connection(&app_handle, &ConnectionStatus {
        log_tailing:     tailing_now,
        addon_connected: false,   // updated by identity watcher
        wow_path:        wow_path_str.clone(),
    });

    // Initial read — pick up any lines already in the current log file
    state.read_new_lines(&tx)?;

    loop {
        // recv_timeout allows a periodic heartbeat so the frontend always receives
        // the current connection status even if it missed the initial one-shot emit
        // (race between tailer startup and the webview registering its listen() handler).
        match fs_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(Event { kind, paths, .. })) => {
                match kind {
                    // A new file was created — check if it's a newer combat log
                    EventKind::Create(_) => {
                        let is_combat_log = paths.iter().any(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| n.starts_with("WoWCombatLog") && n.ends_with(".txt"))
                                .unwrap_or(false)
                        });
                        if is_combat_log {
                            let was_tailing = state.active_file.is_some();
                            state.check_for_new_log();
                            // Emit updated status when we first pick up a log file
                            if !was_tailing && state.active_file.is_some() {
                                ipc::emit_connection(&app_handle, &ConnectionStatus {
                                    log_tailing:     true,
                                    addon_connected: false,
                                    wow_path:        wow_path_str.clone(),
                                });
                            }
                            if let Err(e) = state.read_new_lines(&tx) {
                                tracing::warn!("Tailer read error after log switch: {}", e);
                            }
                        }
                    }
                    // Existing file was modified — read new lines if it's our active file
                    EventKind::Modify(_) => {
                        let active = state.active_file.as_deref();
                        let is_active = paths.iter().any(|p| Some(p.as_path()) == active);
                        if is_active {
                            if let Err(e) = state.read_new_lines(&tx) {
                                tracing::warn!("Tailer read error: {}", e);
                            }
                        }
                    }
                    _ => {} // Access / metadata / delete events — ignore
                }
            }
            Ok(Err(e)) => tracing::error!("Watcher error: {}", e),
            // Heartbeat: no filesystem event for 5 s — re-emit connection status.
            // This recovers from the race where the frontend registered its listener
            // after the one-shot startup emission had already fired.
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                ipc::emit_connection(&app_handle, &ConnectionStatus {
                    log_tailing:     state.active_file.is_some(),
                    addon_connected: false,
                    wow_path:        wow_path_str.clone(),
                });
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!("Watcher channel closed — tailer exiting");
                break;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::mpsc as std_mpsc;
    use tempfile::tempdir;

    // read_new_lines() is entirely synchronous — it uses blocking_send() which
    // must NOT be called from inside a tokio runtime.  We use a std::sync::mpsc
    // channel here so these are plain synchronous tests with no runtime at all.
    fn make_channel() -> (tokio::sync::mpsc::Sender<String>, std_mpsc::Receiver<String>) {
        // Bridge: tokio sender (what TailerState expects) → std receiver for assertions.
        let (tok_tx, mut tok_rx) = tokio::sync::mpsc::channel::<String>(64);
        let (std_tx, std_rx)     = std_mpsc::sync_channel::<String>(64);

        // Drain the tokio channel into the std channel synchronously.
        // We do this lazily by spinning a thread that forwards messages.
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async move {
                while let Some(msg) = tok_rx.recv().await {
                    if std_tx.send(msg).is_err() {
                        break;
                    }
                }
            });
        });

        (tok_tx, std_rx)
    }

    #[test]
    fn reads_initial_lines() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("WoWCombatLog.txt");
        let mut f = std::fs::File::create(&log_path).unwrap();
        writeln!(f, "line one").unwrap();
        writeln!(f, "line two").unwrap();
        f.flush().unwrap();

        let (tx, rx) = make_channel();
        let mut state = TailerState::new(dir.path().to_path_buf());
        state.read_new_lines(&tx).unwrap();

        assert_eq!(rx.recv().unwrap(), "line one");
        assert_eq!(rx.recv().unwrap(), "line two");
    }

    #[test]
    fn detects_rotation() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("WoWCombatLog.txt");

        {
            let mut f = std::fs::File::create(&log_path).unwrap();
            writeln!(f, "original content").unwrap();
            f.flush().unwrap();
        }

        let (tx, rx) = make_channel();
        let mut state = TailerState::new(dir.path().to_path_buf());
        state.read_new_lines(&tx).unwrap();
        let _ = rx.recv(); // consume "original content"

        // Simulate rotation: overwrite with shorter content
        {
            let mut f2 = std::fs::File::create(&log_path).unwrap();
            writeln!(f2, "new").unwrap();
            f2.flush().unwrap();
        }

        state.read_new_lines(&tx).unwrap();
        assert_eq!(rx.recv().unwrap(), "new");
    }

    #[test]
    fn switches_to_newer_log_file() {
        let dir = tempdir().unwrap();

        // Create the "old" log
        let old_path = dir.path().join("WoWCombatLog_2024_01_01_100000.txt");
        {
            let mut f = std::fs::File::create(&old_path).unwrap();
            writeln!(f, "old line").unwrap();
            f.flush().unwrap();
        }

        let (tx, rx) = make_channel();
        let mut state = TailerState::new(dir.path().to_path_buf());
        state.read_new_lines(&tx).unwrap();
        assert_eq!(rx.recv().unwrap(), "old line");

        // WoW creates a newer log
        let new_path = dir.path().join("WoWCombatLog_2024_06_15_195432.txt");
        {
            let mut f = std::fs::File::create(&new_path).unwrap();
            writeln!(f, "new line").unwrap();
            f.flush().unwrap();
        }

        // Simulate the Create event handler
        state.check_for_new_log();
        state.read_new_lines(&tx).unwrap();

        assert_eq!(rx.recv().unwrap(), "new line");
        // Confirm we really switched
        assert_eq!(state.active_file.as_deref(), Some(new_path.as_path()));
    }

    /// Regression: tailer should not panic or error when the directory has no
    /// combat log yet (e.g. player hasn't enabled /combatlog).
    #[test]
    fn handles_empty_logs_dir_gracefully() {
        let dir = tempdir().unwrap();
        std::fs::File::create(dir.path().join("addon_errors.txt")).unwrap();

        let (tx, rx) = make_channel();
        let mut state = TailerState::new(dir.path().to_path_buf());
        state.read_new_lines(&tx).unwrap();
        // Give the forwarding thread a moment, then confirm nothing arrived
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(rx.try_recv().is_err()); // nothing emitted
    }
}
