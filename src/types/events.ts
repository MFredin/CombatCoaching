// TypeScript types mirroring Rust IPC structs.
// These must stay in sync with:
//   src-tauri/src/engine.rs  (AdviceEvent, Severity)
//   src-tauri/src/ipc.rs     (StateSnapshot, ConnectionStatus, event name constants)
//   src-tauri/src/config.rs  (AppConfig, PanelPosition)
//   src-tauri/src/identity.rs (PlayerIdentity)

export type Severity = "good" | "warn" | "bad";

export interface AdviceEvent {
  key:          string;
  title:        string;
  message:      string;
  severity:     Severity;
  kv:           [string, string][];
  timestamp_ms: number;
}

export interface StateSnapshot {
  pull_elapsed_ms: number;
  gcd_gap_ms:      number;
  avoidable_count: number;
  in_combat:       boolean;
}

export interface PlayerIdentity {
  guid:    string;
  name:    string;
  realm:   string;
  class:   string;
  spec:    string;
  version: string;
}

export interface ConnectionStatus {
  log_tailing:     boolean;
  addon_connected: boolean;
  wow_path:        string;
}

export interface PanelPosition {
  id:       string;
  x:        number;
  y:        number;
  visible:  boolean;
  /** Background + text opacity 0–1 (default 1.0) */
  opacity?: number;
  /** Uniform scale factor 0.5–2.0 (default 1.0) */
  scale?:   number;
}

// All fields are optional here because:
// - Rust fills them in with #[serde(default)] before sending to the frontend
// - Spread-updates in the settings UI only supply the field being changed
// - config closures capture AppConfig | null so spreading produces optional fields
export interface AppConfig {
  wow_log_path?:    string;
  addon_sv_path?:   string;
  intensity?:       number;
  player_focus?:    string;
  panel_positions?: PanelPosition[];
  major_cds?:       number[];
  audio_cues?:      AudioCue[];
  hotkeys?:         HotkeyConfig;
  overlay_visible?: boolean;
}

export interface UpdateInfo {
  available:       boolean;
  current_version: string;
  new_version:     string | null;
  notes:           string | null;
}

export interface AudioCue {
  severity:   string;   // "good" | "warn" | "bad"
  enabled:    boolean;
  volume:     number;   // 0.0–1.0
  sound_path: string;   // empty = built-in beep
}

export interface HotkeyConfig {
  toggle_overlay: string; // e.g. "Ctrl+Shift+O", empty = none
}

/// A character found in the WTF directory tree.
/// Mirrors config::WtfCharacter on the Rust side.
export interface WtfCharacter {
  name:    string;
  realm:   string;
  account: string;
}

// IPC event name constants — must match ipc.rs
export const EVENT_ADVICE:     string = "coach:advice";
export const EVENT_STATE:      string = "coach:state";
export const EVENT_CONNECTION: string = "coach:connection";
export const EVENT_IDENTITY:   string = "coach:identity";

// Known panel IDs
export const PANEL_PULL_CLOCK:   string = "pull_clock";
export const PANEL_NOW_FEED:     string = "now_feed";
export const PANEL_TIMELINE:     string = "timeline";
export const PANEL_STAT_WIDGETS: string = "stat_widgets";
