mod config;
mod db;
mod engine;
mod identity;
mod ipc;
mod parser;
mod rules;
mod state;
mod tailer;

use tauri::{Emitter, Manager};
use tauri::dpi::{PhysicalPosition, PhysicalSize};
use tokio::sync::mpsc;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("combat_ledger_lib=debug".parse().unwrap()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
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
