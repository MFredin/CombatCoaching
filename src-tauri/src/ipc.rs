/// IPC bridge — relays Rust engine output to both Tauri frontend windows.
///
/// Tauri v2 API: `use tauri::Emitter; app_handle.emit(event_name, &payload)`
/// This sends to ALL webview windows simultaneously (overlay + settings).
/// Each window subscribes only to the events it needs via `listen()` in TypeScript.
///
/// IMPORTANT: Tauri v2 uses `app_handle.emit()` (not v1's `emit_all()`).
/// The `Emitter` trait must be in scope.
use crate::engine::AdviceEvent;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::Receiver;

// ---------------------------------------------------------------------------
// Event name constants — must match the TypeScript side in src/types/events.ts
// ---------------------------------------------------------------------------
pub const EVENT_ADVICE:     &str = "coach:advice";
pub const EVENT_STATE:      &str = "coach:state";
pub const EVENT_CONNECTION: &str = "coach:connection";
#[allow(dead_code)] // used by TypeScript listener; emitted in future identity phase
pub const EVENT_IDENTITY:   &str = "coach:identity";

// ---------------------------------------------------------------------------
// Payload types (serialised as JSON over the IPC boundary)
// ---------------------------------------------------------------------------

/// Snapshot of the current combat state — sent after every log event.
/// Used by PullClock, StatWidgets, and Timeline in the overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub pull_elapsed_ms: u64,
    pub gcd_gap_ms:      u64,
    pub avoidable_count: u32,
    pub in_combat:       bool,
    /// Successful interrupts cast by the coached player this pull.
    pub interrupt_count: u32,
    /// Active encounter name from ENCOUNTER_START, or None between pulls.
    pub encounter_name:  Option<String>,
}

/// Connection/health status — sent when tailing starts/stops or identity changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub log_tailing:     bool,
    pub addon_connected: bool,
    pub wow_path:        String,
}

// ---------------------------------------------------------------------------
// IPC task
// ---------------------------------------------------------------------------

/// Drains AdviceEvent and StateSnapshot channels, emitting each to all windows.
pub async fn run(
    mut advice_rx: Receiver<AdviceEvent>,
    mut snap_rx:   Receiver<StateSnapshot>,
    app_handle:    AppHandle,
) -> Result<()> {
    loop {
        tokio::select! {
            Some(advice) = advice_rx.recv() => {
                if let Err(e) = app_handle.emit(EVENT_ADVICE, &advice) {
                    tracing::error!("IPC emit advice error: {}", e);
                }
            }
            Some(snap) = snap_rx.recv() => {
                if let Err(e) = app_handle.emit(EVENT_STATE, &snap) {
                    tracing::error!("IPC emit state error: {}", e);
                }
            }
            else => break,
        }
    }
    Ok(())
}

/// Convenience function — emit a connection status update from anywhere
/// that has an AppHandle (called by tailer and identity watcher).
pub fn emit_connection(handle: &AppHandle, status: &ConnectionStatus) {
    if let Err(e) = handle.emit(EVENT_CONNECTION, status) {
        tracing::warn!("Failed to emit connection status: {}", e);
    }
}
