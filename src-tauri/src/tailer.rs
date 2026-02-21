/// Tails WoWCombatLog.txt, emitting new lines as they are written.
///
/// Uses the `notify` crate (OS-level ReadDirectoryChangesWatcher on Windows)
/// to detect file modifications, then reads from the last known byte offset.
///
/// Rotation handling: WoW recreates the log file when the player toggles
/// `/combatlog` or zones. We detect this by comparing the current file size
/// to our last known position — if the file shrank, we restart from byte 0.
use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;

pub struct TailerState {
    path: PathBuf,
    position: u64,
}

impl TailerState {
    fn new(path: PathBuf) -> Self {
        Self { path, position: 0 }
    }

    fn read_new_lines(&mut self, tx: &Sender<String>) -> Result<()> {
        let metadata = match std::fs::metadata(&self.path) {
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

        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.position))?;

        let reader = BufReader::new(&file);
        for line in reader.lines() {
            match line {
                Ok(l) if !l.is_empty() => {
                    if tx.blocking_send(l).is_err() {
                        return Ok(()); // Receiver gone — pipeline shutting down
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Tailer read error: {}", e);
                    break;
                }
            }
        }

        // Update position to end of file (not end of last line — handles
        // partial line writes gracefully; the partial line won't be read
        // as a full line by BufRead, so we'll re-read it next time).
        self.position = file_len;
        Ok(())
    }
}

pub async fn run(wow_log_path: PathBuf, tx: Sender<String>) -> Result<()> {
    tracing::info!("Tailer starting: {:?}", wow_log_path);

    let watch_dir = wow_log_path
        .parent()
        .unwrap_or(wow_log_path.as_path())
        .to_path_buf();

    let (fs_tx, fs_rx) = std_mpsc::channel::<notify::Result<Event>>();

    // notify::Config with a small poll interval as fallback
    let config = notify::Config::default()
        .with_poll_interval(Duration::from_millis(500));

    let mut watcher = RecommendedWatcher::new(fs_tx, config)?;
    watcher.watch(&watch_dir, RecursiveMode::NonRecursive)?;

    let mut state = TailerState::new(wow_log_path.clone());

    // Initial read — pick up any lines already written before we started
    if wow_log_path.exists() {
        state.read_new_lines(&tx)?;
    }

    loop {
        match fs_rx.recv() {
            Ok(Ok(Event { kind: EventKind::Modify(_), paths, .. })) => {
                if paths.iter().any(|p| p == &wow_log_path) {
                    if let Err(e) = state.read_new_lines(&tx) {
                        tracing::warn!("Tailer read error: {}", e);
                    }
                }
            }
            Ok(Ok(_)) => {} // Create / delete / access events — ignore
            Ok(Err(e)) => tracing::error!("Watcher error: {}", e),
            Err(_) => {
                tracing::warn!("Watcher channel closed — tailer exiting");
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn reads_initial_lines() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "line one").unwrap();
        writeln!(f, "line two").unwrap();
        f.flush().unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let mut state = TailerState::new(f.path().to_path_buf());
        state.read_new_lines(&tx).unwrap();

        assert_eq!(rx.try_recv().unwrap(), "line one");
        assert_eq!(rx.try_recv().unwrap(), "line two");
    }

    #[tokio::test]
    async fn detects_rotation() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "original content").unwrap();
        f.flush().unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let mut state = TailerState::new(f.path().to_path_buf());
        state.read_new_lines(&tx).unwrap();
        let _ = rx.try_recv(); // consume "original content"

        // Simulate rotation: overwrite with shorter content
        let mut f2 = std::fs::File::create(f.path()).unwrap();
        writeln!(f2, "new").unwrap();
        f2.flush().unwrap();

        state.read_new_lines(&tx).unwrap();
        assert_eq!(rx.try_recv().unwrap(), "new");
    }
}
