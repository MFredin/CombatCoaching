/// IPC bridge — relays Rust engine output to both Tauri frontend windows.
///
/// Tauri v2 API: `use tauri::Emitter; app_handle.emit(event_name, &payload)`
/// This sends to ALL webview windows simultaneously (overlay + settings).
///
/// NOTE: capabilities files cause a startup crash in this codebase due to a
/// tauri-build (2.5.5) / tauri runtime (2.10.2) version mismatch — the binary
/// format generated at build time is incompatible with the ACL parser at runtime.
/// The emit() calls here are therefore best-effort: they succeed only if a future
/// version aligns tauri-build with the runtime.  The primary delivery path for all
/// events is now managed-state polling via invoke() (get_state_snapshot,
/// drain_advice_queue, get_connection_status) — all confirmed working.
use crate::engine::AdviceEvent;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc::Receiver;

// ---------------------------------------------------------------------------
// Event log queue — distinct type so it can coexist with VecDeque<AdviceEvent>
// in Tauri managed state (different types = different state slots).
// ---------------------------------------------------------------------------

/// Ring-buffered string queue for the Event Feed panel in the settings window.
/// Entries are human-readable formatted strings describing significant events
/// (advice fired, combat transitions, encounter start/end, connection changes).
pub struct EventLogQueue {
    inner: VecDeque<String>,
}

impl EventLogQueue {
    pub fn new() -> Self {
        Self { inner: VecDeque::new() }
    }

    /// Push an entry, capping the buffer at 200 entries.
    pub fn push(&mut self, entry: String) {
        self.inner.push_back(entry);
        if self.inner.len() > 200 {
            self.inner.pop_front();
        }
    }

    /// Drain all entries (atomically clear and return them).
    pub fn drain(&mut self) -> Vec<String> {
        self.inner.drain(..).collect()
    }
}

// ---------------------------------------------------------------------------
// Event name constants — must match the TypeScript side in src/types/events.ts
// ---------------------------------------------------------------------------
pub const EVENT_ADVICE:     &str = "coach:advice";
pub const EVENT_STATE:      &str = "coach:state";
pub const EVENT_CONNECTION: &str = "coach:connection";
#[allow(dead_code)] // used by TypeScript listener; emitted in future identity phase
pub const EVENT_IDENTITY:   &str = "coach:identity";
pub const EVENT_DEBRIEF:    &str = "coach:debrief";

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

/// End-of-pull summary — emitted on every pull end (kill or wipe).
/// Displayed as a 10-second debrief panel on the overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullDebrief {
    pub pull_number:        u32,
    /// How long the pull lasted in milliseconds.
    pub pull_elapsed_ms:    u64,
    /// "kill", "wipe", or "unknown"
    pub outcome:            String,
    /// Total hits from avoidable damage this pull.
    pub avoidable_count:    u32,
    /// Successful interrupts this pull.
    pub interrupt_count:    u32,
    /// Total advice events that fired this pull.
    pub total_advice_fired: u32,
    /// Number of GCD gap advice events that fired this pull.
    pub gcd_gap_count:      u32,
}

// ---------------------------------------------------------------------------
// IPC task
// ---------------------------------------------------------------------------

/// Drains AdviceEvent, StateSnapshot, and PullDebrief channels, emitting each to all
/// windows AND writing to managed state for invoke()-based polling.
///
/// Managed-state side-effects (primary delivery path):
///   • Mutex<StateSnapshot>           — overwritten on every snap; polled via get_state_snapshot
///   • Mutex<VecDeque<AdviceEvent>>   — ring-buffered (cap 50); drained via drain_advice_queue
///
/// emit() calls are best-effort (succeed only if capabilities work); polling is always reliable.
pub async fn run(
    mut advice_rx:  Receiver<AdviceEvent>,
    mut snap_rx:    Receiver<StateSnapshot>,
    mut debrief_rx: Receiver<PullDebrief>,
    app_handle:     AppHandle,
) -> Result<()> {
    // Track previous combat state to detect transitions for the event log.
    let mut prev_in_combat     = false;
    let mut prev_encounter:    Option<String> = None;

    loop {
        tokio::select! {
            Some(advice) = advice_rx.recv() => {
                // Best-effort emit (may silently fail without capabilities)
                let _ = app_handle.emit(EVENT_ADVICE, &advice);
                // Primary delivery: push to managed ring buffer for drain polling
                if let Some(state) = app_handle.try_state::<Mutex<VecDeque<AdviceEvent>>>() {
                    if let Ok(mut q) = state.lock() {
                        q.push_back(advice.clone());
                        if q.len() > 50 { q.pop_front(); } // cap ring buffer at 50
                    }
                }
                // Event log: record each advice event so the Event Feed shows it
                if let Some(eq) = app_handle.try_state::<Mutex<EventLogQueue>>() {
                    if let Ok(mut q) = eq.lock() {
                        let sev_icon = match advice.severity {
                            crate::engine::Severity::Good => "✅",
                            crate::engine::Severity::Warn => "⚠️",
                            crate::engine::Severity::Bad  => "❌",
                        };
                        let ts = chrono_hms(advice.timestamp_ms);
                        q.push(format!("[{}] {} {} — {}", ts, sev_icon, advice.title, advice.message));
                    }
                }
            }
            Some(snap) = snap_rx.recv() => {
                // Best-effort emit
                let _ = app_handle.emit(EVENT_STATE, &snap);
                // Primary delivery: overwrite managed snapshot for poll
                if let Some(state) = app_handle.try_state::<Mutex<StateSnapshot>>() {
                    if let Ok(mut s) = state.lock() {
                        *s = snap.clone();
                    }
                }
                // Event log: combat state transitions + encounter changes
                if let Some(eq) = app_handle.try_state::<Mutex<EventLogQueue>>() {
                    if let Ok(mut q) = eq.lock() {
                        let ts = chrono_hms(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64
                        );
                        // Combat start
                        if !prev_in_combat && snap.in_combat {
                            let name = snap.encounter_name.as_deref().unwrap_or("Open World");
                            q.push(format!("[{}] 🔴 Combat started — {}", ts, name));
                        }
                        // Combat end
                        if prev_in_combat && !snap.in_combat {
                            q.push(format!("[{}] ⚫ Combat ended", ts));
                        }
                        // Encounter name change (new boss / M+ pack)
                        if snap.encounter_name != prev_encounter && snap.encounter_name.is_some() {
                            if let Some(ref enc) = snap.encounter_name {
                                q.push(format!("[{}] 🏰 Encounter: {}", ts, enc));
                            }
                        }
                        prev_in_combat  = snap.in_combat;
                        prev_encounter  = snap.encounter_name;
                    }
                }
            }
            Some(debrief) = debrief_rx.recv() => {
                // Best-effort emit only
                let _ = app_handle.emit(EVENT_DEBRIEF, &debrief);
                // Event log: pull summary
                if let Some(eq) = app_handle.try_state::<Mutex<EventLogQueue>>() {
                    if let Ok(mut q) = eq.lock() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let elapsed_s = debrief.pull_elapsed_ms / 1000;
                        let icon = if debrief.outcome == "kill" { "🏆" } else { "💀" };
                        q.push(format!(
                            "[{}] {} Pull #{} — {} — {}s, {} advice, {} avoidable",
                            chrono_hms(ts), icon,
                            debrief.pull_number, debrief.outcome,
                            elapsed_s, debrief.total_advice_fired, debrief.avoidable_count
                        ));
                    }
                }
            }
            else => break,
        }
    }
    Ok(())
}

/// Format a Unix-epoch millisecond timestamp as "HH:MM:SS" for the event log.
fn chrono_hms(ts_ms: u64) -> String {
    let total_secs = (ts_ms / 1000) % 86_400; // seconds into the day (UTC)
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Convenience function — emit a connection status update from anywhere
/// that has an AppHandle (called by tailer and identity watcher).
///
/// Also updates the `Mutex<ConnectionStatus>` managed state so that
/// `get_connection_status` (called by the frontend on mount) always returns
/// the latest value, even if the webview missed the live event.
pub fn emit_connection(handle: &AppHandle, status: &ConnectionStatus) {
    tracing::debug!(
        "emit_connection: log_tailing={} addon={} path={:?}",
        status.log_tailing, status.addon_connected, status.wow_path
    );
    // Update managed state (best-effort; state registered in lib.rs setup()).
    if let Some(state) = handle.try_state::<Mutex<ConnectionStatus>>() {
        if let Ok(mut guard) = state.lock() {
            let prev = guard.clone();
            *guard = status.clone();
            // Event log: only log when connection status CHANGES (not every heartbeat)
            drop(guard);
            if prev.log_tailing != status.log_tailing || prev.addon_connected != status.addon_connected {
                if let Some(eq) = handle.try_state::<Mutex<EventLogQueue>>() {
                    if let Ok(mut q) = eq.lock() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let log_str = if status.log_tailing { "🟢 Log connected" } else { "🔴 Log disconnected" };
                        let addon_str = if status.addon_connected { " · Addon connected" } else { "" };
                        q.push(format!("[{}] {}{}", chrono_hms(ts), log_str, addon_str));
                    }
                }
            }
        }
    }
    if let Err(e) = handle.emit(EVENT_CONNECTION, status) {
        tracing::warn!("Failed to emit connection status: {}", e);
    }
}
