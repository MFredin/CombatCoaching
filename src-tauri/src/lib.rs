mod config;
mod db;
mod engine;
mod identity;
mod ipc;
mod parser;
mod rules;
mod state;
mod tailer;

use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};
use tokio::sync::mpsc;

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
            tauri_plugin_updater::Builder::new()
                .endpoints(vec![
                    "https://github.com/MFredin/CombatCoaching/releases/latest/download/latest.json"
                        .parse()
                        .expect("valid updater URL"),
                ])
                .build(),
        )
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
            let (raw_tx,  raw_rx)      = mpsc::channel::<String>(2048);
            let (event_tx, event_rx)   = mpsc::channel::<parser::LogEvent>(1024);
            let (advice_tx, advice_rx) = mpsc::channel::<engine::AdviceEvent>(128);
            let (id_tx,   id_rx)       = mpsc::channel::<identity::PlayerIdentity>(16);
            // State snapshots emitted by engine for UI widgets
            let (snap_tx, snap_rx)     = mpsc::channel::<ipc::StateSnapshot>(128);

            // --- SQLite ---
            let db_path = app.path().app_data_dir()?.join("sessions.sqlite");
            let _db = db::init(db_path)?;

            let handle = app.handle().clone();

            // --- If paths are configured, start the pipeline ---
            // On first run paths will be empty; the settings UI wizard saves them.
            if !cfg.wow_log_path.as_os_str().is_empty() {
                let wow_path_str = cfg.wow_log_path.to_string_lossy().to_string();
                start_pipeline(
                    cfg.clone(),
                    handle.clone(),
                    wow_path_str,
                    raw_tx,
                    raw_rx,
                    event_tx,
                    event_rx,
                    advice_tx,
                    id_tx,
                    id_rx,
                    snap_tx,
                );
            } else {
                tracing::info!("No WoW path configured — waiting for first-run setup");
                let _ = handle.emit(ipc::EVENT_CONNECTION, ipc::ConnectionStatus {
                    log_tailing:     false,
                    addon_connected: false,
                    wow_path:        String::new(),
                });
            }

            // IPC task always runs (relays advice + snapshots to frontend)
            let ipc_handle = app.handle().clone();
            tauri::async_runtime::spawn(ipc::run(advice_rx, snap_rx, ipc_handle));

            // Show overlay after setup
            overlay.show()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::get_config,
            config::save_config,
            config::detect_wow_path,
            config::list_wtf_characters,
            check_for_update,
            toggle_overlay,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Spawns all async pipeline tasks.
fn start_pipeline(
    cfg: config::AppConfig,
    app_handle: tauri::AppHandle,
    wow_path_str: String,
    raw_tx: mpsc::Sender<String>,
    raw_rx: mpsc::Receiver<String>,
    event_tx: mpsc::Sender<parser::LogEvent>,
    event_rx: mpsc::Receiver<parser::LogEvent>,
    advice_tx: mpsc::Sender<engine::AdviceEvent>,
    id_tx: mpsc::Sender<identity::PlayerIdentity>,
    id_rx: mpsc::Receiver<identity::PlayerIdentity>,
    snap_tx: mpsc::Sender<ipc::StateSnapshot>,
) {
    let wow_log_path  = cfg.wow_log_path.clone();
    let addon_sv_path = cfg.addon_sv_path.clone();

    tauri::async_runtime::spawn(tailer::run(
        wow_log_path,
        raw_tx,
        app_handle.clone(),
        wow_path_str,
    ));
    tauri::async_runtime::spawn(parser::run(raw_rx, event_tx));
    tauri::async_runtime::spawn(identity::run(addon_sv_path, id_tx, app_handle));
    tauri::async_runtime::spawn(engine::run(event_rx, id_rx, advice_tx, snap_tx, cfg));
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

/// Check GitHub Releases for a newer version.
/// Returns UpdateInfo; if an update is available the frontend shows a prompt.
/// Downloading and installing is handled by tauri-plugin-updater automatically.
#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    use tauri_plugin_updater::UpdaterExt;

    let current = app.package_info().version.to_string();

    match app.updater() {
        Ok(updater) => {
            match updater.check().await {
                Ok(Some(update)) => {
                    tracing::info!(
                        "Update available: {} → {}",
                        current,
                        update.version
                    );
                    Ok(UpdateInfo {
                        available:       true,
                        current_version: current,
                        new_version:     Some(update.version.clone()),
                        notes:           update.body.clone(),
                    })
                }
                Ok(None) => {
                    tracing::info!("No update available (current: {})", current);
                    Ok(UpdateInfo {
                        available:       false,
                        current_version: current,
                        new_version:     None,
                        notes:           None,
                    })
                }
                Err(e) => {
                    tracing::warn!("Update check failed: {}", e);
                    Err(format!("Update check failed: {}", e))
                }
            }
        }
        Err(e) => {
            tracing::warn!("Updater not available: {}", e);
            Err(format!("Updater not configured: {}", e))
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

fn invoke_save(cfg: &config::AppConfig, config_dir: &std::path::Path) -> anyhow::Result<()> {
    let raw = toml::to_string_pretty(cfg)
        .map_err(|e| anyhow::anyhow!("Config serialize error: {}", e))?;
    std::fs::write(config_dir.join("config.toml"), raw)?;
    Ok(())
}
