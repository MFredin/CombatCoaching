mod config;
mod db;
mod engine;
mod identity;
mod ipc;
mod parser;
mod rules;
mod specs;
mod state;
mod tailer;

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Manager, PhysicalPosition, PhysicalSize};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Pipeline state — stored in Tauri managed state so try_start_pipeline() can
// be called from both setup() (on startup) and save_config (on first-run save).
// ---------------------------------------------------------------------------

/// All channel ends + db handle needed to start the pipeline.
/// Wrapped in `Mutex<Option<…>>` so `Option::take()` in try_start_pipeline
/// ensures we only ever spawn the pipeline once.
struct PipelineBundle {
    raw_tx:     mpsc::Sender<String>,
    raw_rx:     mpsc::Receiver<String>,
    event_tx:   mpsc::Sender<parser::LogEvent>,
    event_rx:   mpsc::Receiver<parser::LogEvent>,
    id_tx:      mpsc::Sender<identity::PlayerIdentity>,
    id_rx:      mpsc::Receiver<identity::PlayerIdentity>,
    advice_tx:  mpsc::Sender<engine::AdviceEvent>,
    advice_rx:  mpsc::Receiver<engine::AdviceEvent>,
    snap_tx:    mpsc::Sender<ipc::StateSnapshot>,
    snap_rx:    mpsc::Receiver<ipc::StateSnapshot>,
    debrief_tx: mpsc::Sender<ipc::PullDebrief>,
    debrief_rx: mpsc::Receiver<ipc::PullDebrief>,
    db_writer:  db::DbWriter,
}

pub fn run() {
    // -----------------------------------------------------------------------
    // Logging — write to both stderr (debug) and a rolling log file.
    // Log directory: <AppData>\Roaming\com.combatledger.livecoach\logs\
    // Files rotate daily; we keep the last 7.
    // -----------------------------------------------------------------------
    //
    // We initialise logging here before the Tauri builder so any panic during
    // setup is captured in the log file.
    //
    // NOTE: app_log_dir() is not available before the builder runs, so we
    // derive the path manually using the known Windows APPDATA env var.
    // Tauri's identifier is "com.combatledger.livecoach".
    let log_dir = {
        let base = std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        base.join("com.combatledger.livecoach").join("logs")
    };
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "coach.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Keep _guard alive for the process lifetime — drop = flush
    // We leak it intentionally; it lives as long as the app does.
    std::mem::forget(_guard);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("combat_ledger_lib=debug".parse().unwrap()),
        )
        .with_writer(non_blocking)
        .with_ansi(false) // log files should not contain ANSI colour codes
        .init();

    // -----------------------------------------------------------------------
    // Panic hook — log panics through tracing before the process dies.
    // Without this, panic messages only appear on stderr (invisible in prod).
    // -----------------------------------------------------------------------
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_string()
        };
        tracing::error!("PANIC at {}: {}", location, message);
    }));

    tracing::info!("CombatLedger Live Coach starting — logs → {}", log_dir.display());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            // v2.3.1 API: handler is registered at build time; register() only
            // takes the shortcut with no callback.
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    use tauri_plugin_global_shortcut::ShortcutState;
                    if event.state() == ShortcutState::Pressed {
                        if let Some(ov) = app.get_webview_window("overlay") {
                            let vis = ov.is_visible().unwrap_or(false);
                            if vis { let _ = ov.hide(); } else { let _ = ov.show(); }
                        }
                    }
                })
                .build()
        )
        // tauri-plugin-updater intentionally omitted — requires a signing key pair.
        // Update checks use the check_for_update command below (GitHub API via reqwest).
        // TODO: generate a keypair and re-enable tauri-plugin-updater for auto-install.
        .setup(|app| {
            // --- Overlay window: make it transparent and click-through ---
            let overlay = app.get_webview_window("overlay").expect("overlay window not found");
            overlay.set_ignore_cursor_events(true)?;

            // --- Resize overlay to cover the primary monitor exactly ---
            // tauri.conf.json hardcodes 1920x1080 as a safe fallback; we override
            // at runtime so high-DPI, ultrawide, and non-1080p monitors are covered.
            if let Ok(Some(monitor)) = overlay.current_monitor() {
                let size = monitor.size();
                let pos  = monitor.position();
                tracing::info!(
                    "Overlay monitor: {}x{} at ({},{})",
                    size.width, size.height, pos.x, pos.y
                );
                let _ = overlay.set_size(PhysicalSize::new(size.width, size.height));
                let _ = overlay.set_position(PhysicalPosition::new(pos.x, pos.y));
            } else {
                tracing::warn!("Could not detect monitor size — overlay uses conf.json defaults");
            }

            // --- Load config (or create default on first run) ---
            let config_dir = app.path().app_config_dir()?;
            let cfg = config::load_or_default(&config_dir)?;

            // --- Build inter-module async channels ---
            // Pipeline: tailer -> parser -> engine -> ipc
            // All channel ends are bundled together and stored in managed state.
            // try_start_pipeline() takes the bundle and spawns all tasks atomically,
            // so ipc::run is never live without its corresponding senders being held
            // by the engine/tailer/identity tasks.
            let (raw_tx,     raw_rx)     = mpsc::channel::<String>(2048);
            let (event_tx,   event_rx)   = mpsc::channel::<parser::LogEvent>(1024);
            let (advice_tx,  advice_rx)  = mpsc::channel::<engine::AdviceEvent>(128);
            let (id_tx,      id_rx)      = mpsc::channel::<identity::PlayerIdentity>(16);
            let (snap_tx,    snap_rx)    = mpsc::channel::<ipc::StateSnapshot>(128);
            let (debrief_tx, debrief_rx) = mpsc::channel::<ipc::PullDebrief>(16);

            // --- SQLite ---
            let db_path  = app.path().app_data_dir()?.join("sessions.sqlite");
            let db_writer = db::spawn_db_writer(&db_path)?;

            // --- Store bundle + ready-flag in managed state ---
            let bundle = PipelineBundle {
                raw_tx, raw_rx,
                event_tx, event_rx,
                id_tx, id_rx,
                advice_tx, advice_rx,
                snap_tx, snap_rx,
                debrief_tx, debrief_rx,
                db_writer,
            };
            app.manage(Mutex::new(Some(bundle)));
            app.manage(AtomicBool::new(false)); // pipeline-running gate
            // Latest connection status — kept in sync by ipc::emit_connection()
            // so the frontend can poll get_connection_status on mount and always
            // get the correct value even if it missed the live event.
            app.manage(Mutex::new(ipc::ConnectionStatus {
                log_tailing: false, addon_connected: false, wow_path: String::new()
            }));

            let handle = app.handle().clone();

            // --- Register global hotkey from config ---
            register_global_hotkey(&handle, &cfg.hotkeys.toggle_overlay);

            // --- If path is already configured, start the pipeline immediately ---
            // On first run the path is empty; it will be set by the settings wizard.
            // The save_config command calls try_start_pipeline after persisting the path.
            if !cfg.wow_log_path.as_os_str().is_empty() {
                try_start_pipeline(&handle);
            } else {
                tracing::info!("No WoW path configured — waiting for first-run setup");
            }

            // Show overlay after setup
            overlay.show()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::get_config,
            save_config,
            get_connection_status,
            config::detect_wow_path,
            config::list_wtf_characters,
            config::list_specs,
            config::apply_spec,
            check_for_update,
            toggle_overlay,
            get_pull_history,
            read_audio_file,
            register_hotkey,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Try to start the async pipeline tasks.
///
/// This is a no-op if:
/// - `wow_log_path` is not yet configured (returns immediately; called again after save)
/// - The pipeline is already running (`AtomicBool` CAS prevents double-start)
///
/// On first call with a valid path, takes the `PipelineBundle` from managed state
/// (can only happen once) and spawns all 5 tasks together — including `ipc::run` —
/// so the IPC relay is always live alongside the tasks that feed it.
fn try_start_pipeline(app: &tauri::AppHandle) {
    // Re-read config from disk so we get the path saved most recently by save_config.
    let config_dir = match app.path().app_config_dir() {
        Ok(d) => d,
        Err(e) => { tracing::error!("try_start_pipeline: cannot resolve config dir: {}", e); return; }
    };
    let cfg = match config::load_or_default(&config_dir) {
        Ok(c) => c,
        Err(e) => { tracing::error!("try_start_pipeline: config load failed: {}", e); return; }
    };

    if cfg.wow_log_path.as_os_str().is_empty() {
        tracing::info!("try_start_pipeline: wow_log_path not yet set — skipping");
        return;
    }

    // CAS: mark pipeline as running.  If it was already true, another call beat us.
    let ready: tauri::State<AtomicBool> = app.state();
    if ready.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        tracing::info!("try_start_pipeline: pipeline already running — skipping");
        return;
    }

    // Take the bundle (single-use — None after this point).
    let bundle_state: tauri::State<Mutex<Option<PipelineBundle>>> = app.state();
    let bundle = {
        let mut guard = bundle_state.lock().unwrap();
        guard.take()
    };
    let Some(b) = bundle else {
        tracing::warn!("try_start_pipeline: bundle already consumed (double-start prevented)");
        return;
    };

    let wow_path_str = cfg.wow_log_path.to_string_lossy().to_string();
    let h = app.clone();

    tauri::async_runtime::spawn(tailer::run(cfg.wow_log_path.clone(), b.raw_tx, h.clone(), wow_path_str));
    tauri::async_runtime::spawn(parser::run(b.raw_rx, b.event_tx));
    tauri::async_runtime::spawn(identity::run(cfg.addon_sv_path.clone(), b.id_tx, h.clone()));
    tauri::async_runtime::spawn(engine::run(b.event_rx, b.id_rx, b.advice_tx, b.snap_tx, b.debrief_tx, cfg, b.db_writer));
    tauri::async_runtime::spawn(ipc::run(b.advice_rx, b.snap_rx, b.debrief_rx, h));

    tracing::info!("Pipeline started successfully");
}

// ---------------------------------------------------------------------------
// get_connection_status — polled by the frontend on mount (and optionally
// after save) to get the current connection status without relying on the
// one-shot startup event that may have fired before listeners were registered.
// ---------------------------------------------------------------------------

/// Return the latest connection status stored in managed state.
/// `ipc::emit_connection()` keeps this in sync every time the tailer or
/// identity module emits a status change.
#[tauri::command]
fn get_connection_status(app: tauri::AppHandle) -> ipc::ConnectionStatus {
    app.state::<Mutex<ipc::ConnectionStatus>>()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| ipc::ConnectionStatus {
            log_tailing: false, addon_connected: false, wow_path: String::new()
        })
}

// ---------------------------------------------------------------------------
// save_config — wraps config::save + try_start_pipeline so the pipeline
// starts automatically the first time the user sets their WoW log path,
// without requiring an app restart.
// ---------------------------------------------------------------------------

/// Save the settings config to disk, then attempt to start the pipeline.
/// Replaces `config::save_config` in the invoke_handler so try_start_pipeline
/// is always called after a successful save.
#[tauri::command]
fn save_config(app: tauri::AppHandle, config: config::AppConfig) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    config::save(&config, &dir).map_err(|e| e.to_string())?;
    try_start_pipeline(&app);
    Ok(())
}

// ---------------------------------------------------------------------------
// Updater command — called by the frontend's "Check for Updates" button
// and on a background timer at startup.
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct UpdateInfo {
    pub available:       bool,
    pub current_version: String,
    pub new_version:     Option<String>,
    pub notes:           Option<String>,
}

/// Check GitHub Releases for a newer version by fetching latest.json.
/// Uses the standard GitHub Releases download URL — no plugin required.
/// Version comparison: if the remote version string differs from the current
/// package version, we report an update as available.
#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();

    // Fetch the latest.json manifest uploaded by CI alongside each release.
    let url = "https://github.com/MFredin/CombatCoaching/releases/latest/download/latest.json";

    let response = tauri::async_runtime::spawn_blocking(|| {
        // Use ureq (bundled with tauri as a transitive dep via tauri-utils) for a
        // simple synchronous HTTP GET. ureq is lighter than reqwest for a one-shot check.
        ureq::get(url)
            .call()
            .map(|r| r.into_string().unwrap_or_default())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?;

    match response {
        Err(e) => {
            tracing::warn!("Update check HTTP error: {}", e);
            Err(format!("Update check failed: {}", e))
        }
        Ok(body) => {
            // latest.json shape: { "version": "0.7.0", "notes": "...", ... }
            let parsed: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("Update manifest parse error: {}", e))?;

            let remote_version = parsed["version"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let notes = parsed["notes"].as_str().map(|s| s.to_string());

            let available = !remote_version.is_empty() && remote_version != current;

            tracing::info!(
                "Update check: current={} remote={} available={}",
                current, remote_version, available
            );

            Ok(UpdateInfo {
                available,
                current_version: current,
                new_version:     if available { Some(remote_version) } else { None },
                notes,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Overlay visibility toggle — called by the frontend hotkey button and by
// the global hotkey handler (future: tauri-plugin-global-shortcut).
// ---------------------------------------------------------------------------

/// Show or hide the overlay window. Persists the new state to config so it
/// survives restarts. Returns the new visibility state (true = visible).
#[tauri::command]
fn toggle_overlay(app: tauri::AppHandle) -> Result<bool, String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or_else(|| "Overlay window not found".to_string())?;

    let currently_visible = overlay.is_visible().map_err(|e| e.to_string())?;
    let new_visible = !currently_visible;

    if new_visible {
        overlay.show().map_err(|e| e.to_string())?;
    } else {
        overlay.hide().map_err(|e| e.to_string())?;
    }

    tracing::info!("Overlay toggled: visible={}", new_visible);

    // Persist to config
    if let Ok(config_dir) = app.path().app_config_dir() {
        if let Ok(mut cfg) = config::load_or_default(&config_dir) {
            cfg.overlay_visible = new_visible;
            let _ = invoke_save(&cfg, &config_dir);
        }
    }

    Ok(new_visible)
}

// ---------------------------------------------------------------------------
// Global hotkey helpers
// ---------------------------------------------------------------------------

/// Convert a user-recorded combo string (e.g. "Ctrl+Shift+O") into a typed
/// `tauri_plugin_global_shortcut::Shortcut`.
///
/// Supported modifiers: Ctrl, Shift, Alt, Meta/Win/Super
/// Supported keys:      A-Z, F1-F12
///
/// Returns Err if the combo contains an unsupported token.
fn user_combo_to_shortcut(
    combo: &str,
) -> Result<tauri_plugin_global_shortcut::Shortcut, String> {
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    let mut mods: Modifiers  = Modifiers::empty();
    let mut code: Option<Code> = None;

    for part in combo.split('+') {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control"       => mods |= Modifiers::CONTROL,
            "shift"                  => mods |= Modifiers::SHIFT,
            "alt"                    => mods |= Modifiers::ALT,
            "meta" | "win" | "super" => mods |= Modifiers::SUPER,
            k if k.len() == 1 => {
                code = Some(match k.to_ascii_uppercase().as_str() {
                    "A" => Code::KeyA, "B" => Code::KeyB, "C" => Code::KeyC,
                    "D" => Code::KeyD, "E" => Code::KeyE, "F" => Code::KeyF,
                    "G" => Code::KeyG, "H" => Code::KeyH, "I" => Code::KeyI,
                    "J" => Code::KeyJ, "K" => Code::KeyK, "L" => Code::KeyL,
                    "M" => Code::KeyM, "N" => Code::KeyN, "O" => Code::KeyO,
                    "P" => Code::KeyP, "Q" => Code::KeyQ, "R" => Code::KeyR,
                    "S" => Code::KeyS, "T" => Code::KeyT, "U" => Code::KeyU,
                    "V" => Code::KeyV, "W" => Code::KeyW, "X" => Code::KeyX,
                    "Y" => Code::KeyY, "Z" => Code::KeyZ,
                    _ => return Err(format!("Unsupported key '{}'", k)),
                });
            }
            k => {
                code = Some(match k {
                    "f1"  => Code::F1,  "f2"  => Code::F2,  "f3"  => Code::F3,
                    "f4"  => Code::F4,  "f5"  => Code::F5,  "f6"  => Code::F6,
                    "f7"  => Code::F7,  "f8"  => Code::F8,  "f9"  => Code::F9,
                    "f10" => Code::F10, "f11" => Code::F11, "f12" => Code::F12,
                    _ => return Err(format!("Unsupported token '{}'", k)),
                });
            }
        }
    }

    let c = code.ok_or_else(|| format!("No key specified in combo '{}'", combo))?;
    Ok(Shortcut::new(if mods.is_empty() { None } else { Some(mods) }, c))
}

/// Register (or clear) the overlay-toggle global hotkey.
/// Unregisters all existing hotkeys first to prevent duplicates on re-call.
fn register_global_hotkey(app: &tauri::AppHandle, combo: &str) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    if let Err(e) = app.global_shortcut().unregister_all() {
        tracing::warn!("Hotkey unregister_all error: {}", e);
    }

    if combo.is_empty() {
        tracing::info!("Global hotkey cleared");
        return;
    }

    match user_combo_to_shortcut(combo) {
        Err(e) => tracing::warn!("Invalid hotkey combo '{}': {}", combo, e),
        Ok(shortcut) => {
            // v2.3.1: register() takes only the shortcut; the handler was
            // supplied to Builder::with_handler() at plugin construction time.
            if let Err(e) = app.global_shortcut().register(shortcut) {
                tracing::warn!("Hotkey register failed for '{}': {}", combo, e);
            } else {
                tracing::info!("Global hotkey registered: {}", combo);
            }
        }
    }
}

/// Re-register the overlay toggle hotkey from the settings window.
/// Called after the user records a new combo and saves it.
/// Passing an empty combo clears the hotkey.
#[tauri::command]
fn register_hotkey(app: tauri::AppHandle, combo: String) -> Result<(), String> {
    register_global_hotkey(&app, &combo);
    Ok(())
}

// ---------------------------------------------------------------------------
// Shell helper — open a URL in the user's default browser
// ---------------------------------------------------------------------------

/// Open a URL (or file) in the default browser / associated application.
/// Uses the Windows `start` command so no extra crate is required.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open URL: {}", e))?;
    #[cfg(not(target_os = "windows"))]
    let _ = url; // cross-platform stub — app only ships on Windows
    Ok(())
}

// ---------------------------------------------------------------------------
// Pull history — read-only query, opens its own short-lived SQLite connection
// so the writer thread is never blocked.
// ---------------------------------------------------------------------------

/// One row returned by get_pull_history.
/// Mirrors the joined pulls + sessions + advice_events query.
#[derive(serde::Serialize)]
struct PullHistoryRow {
    pull_id:      i64,
    session_id:   i64,
    pull_number:  u32,
    /// Unix epoch milliseconds (matches the u64 stored by the writer).
    started_at:   u64,
    ended_at:     Option<u64>,
    outcome:      Option<String>,
    encounter:    Option<String>,
    player_name:  String,
    advice_count: u32,
}

/// Return the last 25 pulls (newest first) with advice event counts.
/// Opens a read-only SQLite connection so the writer thread is never blocked.
#[tauri::command]
async fn get_pull_history(app: tauri::AppHandle) -> Result<Vec<PullHistoryRow>, String> {
    let db_path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("sessions.sqlite");

    if !db_path.exists() {
        return Ok(vec![]);
    }

    tauri::async_runtime::spawn_blocking(move || {
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("DB open: {}", e))?;

        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.session_id, p.pull_number, p.started_at, p.ended_at, \
                        p.outcome, p.encounter, \
                        COALESCE(s.player_name, '') AS player_name, \
                        COUNT(ae.id) AS advice_count \
                 FROM pulls p \
                 LEFT JOIN sessions s ON s.id = p.session_id \
                 LEFT JOIN advice_events ae ON ae.pull_id = p.id \
                 GROUP BY p.id \
                 ORDER BY p.id DESC \
                 LIMIT 25",
            )
            .map_err(|e| format!("DB prepare: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                let ended_raw: Option<i64> = row.get(4)?;
                Ok(PullHistoryRow {
                    pull_id:      row.get(0)?,
                    session_id:   row.get(1)?,
                    pull_number:  row.get::<_, i64>(2)? as u32,
                    started_at:   row.get::<_, i64>(3)? as u64,
                    ended_at:     ended_raw.map(|v| v as u64),
                    outcome:      row.get(5)?,
                    encounter:    row.get(6)?,
                    player_name:  row.get(7)?,
                    advice_count: row.get::<_, i64>(8)? as u32,
                })
            })
            .map_err(|e| format!("DB query: {}", e))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("DB row: {}", e))
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

// ---------------------------------------------------------------------------
// Audio file loader — read raw bytes from the filesystem so the overlay's
// Web Audio API can decode them without needing the Tauri asset protocol.
// ---------------------------------------------------------------------------

/// Read a local audio file and return its raw bytes.
/// The overlay converts the returned byte array to an ArrayBuffer and calls
/// `AudioContext.decodeAudioData()` to produce a reusable AudioBuffer.
#[tauri::command]
async fn read_audio_file(path: String) -> Result<Vec<u8>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::read(&path).map_err(|e| format!("Failed to read audio file: {}", e))
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

fn invoke_save(cfg: &config::AppConfig, config_dir: &std::path::Path) -> anyhow::Result<()> {
    let raw = toml::to_string_pretty(cfg)
        .map_err(|e| anyhow::anyhow!("Config serialize error: {}", e))?;
    std::fs::write(config_dir.join("config.toml"), raw)?;
    Ok(())
}
