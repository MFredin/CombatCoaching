/// Application configuration — persisted as TOML in the platform config directory.
///
/// On Windows this is typically:
///   %APPDATA%\com.combatledger.livecoach\config.toml
///   (e.g. C:\Users\<user>\AppData\Roaming\com.combatledger.livecoach\config.toml)
///
/// First-run: if wow_log_path is empty, the settings window shows a wizard
/// that calls detect_wow_path and/or opens a directory picker.
///
/// NOTE: wow_log_path stores the **Logs directory** (e.g. `..\World of Warcraft\_retail_\Logs`),
/// NOT a specific file. The tailer resolves the newest WoWCombatLog*.txt at runtime.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager; // required for AppHandle::path() and app_config_dir()

// ---------------------------------------------------------------------------
// AppConfig
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Audio cues
// ---------------------------------------------------------------------------

/// Per-severity audio cue configuration.
/// Each severity ("good", "warn", "bad") can have its own sound file and volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioCue {
    /// Severity this cue applies to: "good", "warn", or "bad"
    pub severity: String,
    /// Whether this cue is enabled
    #[serde(default = "bool_true")]
    pub enabled: bool,
    /// Volume 0.0–1.0
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Optional path to a custom .wav/.mp3 file; empty = use built-in beep
    #[serde(default)]
    pub sound_path: String,
}

fn bool_true() -> bool { true }
fn default_volume() -> f32 { 0.7 }

fn default_audio_cues() -> Vec<AudioCue> {
    vec![
        AudioCue { severity: "good".to_owned(), enabled: true,  volume: 0.6, sound_path: String::new() },
        AudioCue { severity: "warn".to_owned(), enabled: true,  volume: 0.7, sound_path: String::new() },
        AudioCue { severity: "bad".to_owned(),  enabled: true,  volume: 0.8, sound_path: String::new() },
    ]
}

// ---------------------------------------------------------------------------
// Hotkeys
// ---------------------------------------------------------------------------

/// Global hotkey configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Key combo string for toggling overlay visibility (e.g. "Ctrl+Shift+O").
    /// Empty string = no hotkey registered.
    #[serde(default)]
    pub toggle_overlay: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self { toggle_overlay: String::new() }
    }
}

// ---------------------------------------------------------------------------
// Panel positions
// ---------------------------------------------------------------------------

/// Position, visibility, and appearance of a single overlay panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelPosition {
    /// Panel identifier — matches the known panel IDs in the overlay.
    /// Known IDs: "now_feed", "pull_clock", "timeline", "stat_widgets"
    pub id:      String,
    pub x:       i32,
    pub y:       i32,
    pub visible: bool,
    /// Background + text opacity 0.0–1.0 (default 1.0 = fully visible)
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Uniform scale factor 0.5–2.0 (default 1.0 = native size)
    #[serde(default = "default_scale")]
    pub scale:   f32,
}

fn default_opacity() -> f32 { 1.0 }
fn default_scale()   -> f32 { 1.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Absolute path to the WoW Logs directory (e.g. `..\World of Warcraft\_retail_\Logs`).
    /// The tailer picks the newest WoWCombatLog*.txt in this directory at runtime,
    /// and switches automatically when WoW creates a new timestamped log file.
    #[serde(default)]
    pub wow_log_path: PathBuf,

    /// Absolute path to the addon SavedVariables file (CombatCoach.lua)
    #[serde(default)]
    pub addon_sv_path: PathBuf,

    /// Coaching intensity 1 (quiet) – 5 (aggressive)
    #[serde(default = "default_intensity")]
    pub intensity: u8,

    /// Name of the player to coach (empty = auto from identity handshake)
    #[serde(default)]
    pub player_focus: String,

    /// Overlay panel positions (set in the layout editor)
    #[serde(default = "default_panel_positions")]
    pub panel_positions: Vec<PanelPosition>,

    /// Spell IDs of major cooldowns for the coached player's spec.
    /// Loaded from data/specs/<class>_<spec>.toml when the user selects a spec.
    #[serde(default)]
    pub major_cds: Vec<u32>,

    /// Per-severity audio cue settings.
    #[serde(default = "default_audio_cues")]
    pub audio_cues: Vec<AudioCue>,

    /// Global hotkey bindings.
    #[serde(default)]
    pub hotkeys: HotkeyConfig,

    /// Whether the overlay is currently visible (persisted so it survives restart).
    #[serde(default = "bool_true")]
    pub overlay_visible: bool,
}

fn default_intensity() -> u8 { 3 }

fn default_panel_positions() -> Vec<PanelPosition> {
    vec![
        PanelPosition { id: "pull_clock".to_owned(),   x: 20,  y: 20,  visible: true, opacity: 1.0, scale: 1.0 },
        PanelPosition { id: "now_feed".to_owned(),     x: 20,  y: 70,  visible: true, opacity: 1.0, scale: 1.0 },
        PanelPosition { id: "timeline".to_owned(),     x: 20,  y: 500, visible: true, opacity: 1.0, scale: 1.0 },
        PanelPosition { id: "stat_widgets".to_owned(), x: 20,  y: 670, visible: true, opacity: 1.0, scale: 1.0 },
    ]
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            wow_log_path:    PathBuf::new(),
            addon_sv_path:   PathBuf::new(),
            intensity:       default_intensity(),
            player_focus:    String::new(),
            panel_positions: default_panel_positions(),
            major_cds:       Vec::new(),
            audio_cues:      default_audio_cues(),
            hotkeys:         HotkeyConfig::default(),
            overlay_visible: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Load / save
// ---------------------------------------------------------------------------

pub fn load_or_default(config_dir: &Path) -> Result<AppConfig> {
    let path = config_dir.join("config.toml");
    if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        let cfg: AppConfig = toml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("Config parse error: {}", e))?;
        Ok(cfg)
    } else {
        Ok(AppConfig::default())
    }
}

fn save(config: &AppConfig, config_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let raw = toml::to_string_pretty(config)
        .map_err(|e| anyhow::anyhow!("Config serialize error: {}", e))?;
    std::fs::write(config_dir.join("config.toml"), raw)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands (called from the settings window via invoke())
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_config(app_handle: tauri::AppHandle) -> Result<AppConfig, String> {
    let dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?;
    load_or_default(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_config(app_handle: tauri::AppHandle, config: AppConfig) -> Result<(), String> {
    let dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?;
    save(&config, &dir).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Log directory helpers (used by tailer.rs and detect_wow_path)
// ---------------------------------------------------------------------------

/// Scans `logs_dir` for `WoWCombatLog*.txt` files and returns the path of the
/// one with the most recent `modified` timestamp.  Returns `None` if the
/// directory is empty or contains no matching files.
pub fn find_latest_log(logs_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(logs_dir).ok()?;

    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Match WoWCombatLog*.txt  (case-insensitive on Windows, but WoW uses
        // consistent casing so a simple prefix/suffix check is fine.)
        if !name_str.starts_with("WoWCombatLog") || !name_str.ends_with(".txt") {
            continue;
        }

        let path = entry.path();
        let modified = match entry.metadata().and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };

        match &best {
            None => best = Some((path, modified)),
            Some((_, best_time)) if modified > *best_time => best = Some((path, modified)),
            _ => {}
        }
    }

    if let Some((ref p, _)) = best {
        tracing::debug!("find_latest_log: selected {:?}", p);
    }
    best.map(|(p, _)| p)
}

// ---------------------------------------------------------------------------
// WTF character enumeration
// ---------------------------------------------------------------------------

/// A character found in the WTF directory tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WtfCharacter {
    /// The character name (folder name under Realm/)
    pub name:    String,
    /// The realm name (folder name under Account/<ACCOUNT>/)
    pub realm:   String,
    /// The account folder name (e.g. "12345678#1" or "ACCOUNT_NAME")
    pub account: String,
}

/// Scan the WTF/Account directory tree for characters.
///
/// WTF directory layout:
///   <WoW root>/WTF/Account/<ACCOUNT>/<REALM>/<CHARACTER>/
///
/// We derive the WoW root from `logs_dir` by walking up three levels:
///   logs_dir  = <WoW root>/_retail_/Logs  OR  <WoW root>/Logs
///
/// We try both the `_retail_` subdirectory layout and the flat layout.
pub fn scan_wtf_characters(logs_dir: &Path) -> Vec<WtfCharacter> {
    // Try to find the WTF/Account dir by walking up from the Logs dir.
    // Logs dir can be:
    //   <install>/_retail_/Logs  → WTF is at <install>/_retail_/WTF
    //   <install>/Logs            → WTF is at <install>/WTF
    let candidates: Vec<PathBuf> = {
        let mut v = Vec::new();
        if let Some(parent) = logs_dir.parent() {
            // <parent>/WTF/Account  (e.g. _retail_/WTF/Account)
            v.push(parent.join("WTF").join("Account"));
        }
        v
    };

    let mut characters = Vec::new();

    for account_root in candidates {
        if !account_root.is_dir() {
            continue;
        }

        // Iterate account folders (numeric Battle.net IDs or legacy names)
        let accounts = match std::fs::read_dir(&account_root) {
            Ok(r) => r,
            Err(_) => continue,
        };

        for account_entry in accounts.flatten() {
            if !account_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let account_name = account_entry.file_name().to_string_lossy().to_string();

            // Iterate realm folders inside the account folder
            let realms = match std::fs::read_dir(account_entry.path()) {
                Ok(r) => r,
                Err(_) => continue,
            };

            for realm_entry in realms.flatten() {
                if !realm_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let realm_name = realm_entry.file_name().to_string_lossy().to_string();

                // Skip known non-realm directories that live alongside realm folders
                // (e.g. "SavedVariables", "macros-cache.txt")
                if realm_name == "SavedVariables" || realm_name.contains('.') {
                    continue;
                }

                // Iterate character folders inside the realm folder
                let chars = match std::fs::read_dir(realm_entry.path()) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                for char_entry in chars.flatten() {
                    if !char_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    let char_name = char_entry.file_name().to_string_lossy().to_string();

                    characters.push(WtfCharacter {
                        name:    char_name,
                        realm:   realm_name.clone(),
                        account: account_name.clone(),
                    });
                }
            }
        }

        // If we found characters, don't try more candidate paths
        if !characters.is_empty() {
            break;
        }
    }

    characters.sort_by(|a, b| a.realm.cmp(&b.realm).then(a.name.cmp(&b.name)));
    characters
}

/// Tauri command: returns all WTF characters derived from the configured
/// Logs directory.  Returns an empty list if the directory isn't set or
/// the WTF tree can't be found.
#[tauri::command]
pub fn list_wtf_characters(app_handle: tauri::AppHandle) -> Vec<WtfCharacter> {
    let dir = match app_handle.path().app_config_dir() {
        Ok(d) => d,
        Err(_) => return vec![],
    };
    let cfg = match load_or_default(&dir) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    if cfg.wow_log_path.as_os_str().is_empty() {
        return vec![];
    }
    let chars = scan_wtf_characters(&cfg.wow_log_path);
    tracing::info!("WTF scan found {} characters", chars.len());
    chars
}

/// Try to auto-detect the WoW Logs directory from common install locations.
/// Returns the **directory** path (not a specific file) so the tailer can
/// track whichever WoWCombatLog*.txt file is newest.
#[tauri::command]
pub fn detect_wow_path() -> Option<String> {
    // Common Battle.net install directories across drive letters
    let roots = ["C", "D", "E", "F", "G"];
    let suffixes = [
        r"Program Files (x86)\World of Warcraft\_retail_\Logs",
        r"Program Files (x86)\World of Warcraft\Logs",
        r"Program Files\World of Warcraft\_retail_\Logs",
        r"Program Files\World of Warcraft\Logs",
        r"World of Warcraft\_retail_\Logs",
        r"World of Warcraft\Logs",
        r"Games\World of Warcraft\_retail_\Logs",
    ];

    for root in &roots {
        for suffix in &suffixes {
            let dir = PathBuf::from(format!(r"{}:\{}", root, suffix));
            if dir.is_dir() {
                // Only accept if the directory actually contains a combat log
                if find_latest_log(&dir).is_some() {
                    tracing::info!("Auto-detected WoW Logs dir: {:?}", dir);
                    return Some(dir.to_string_lossy().to_string());
                }
            }
        }
    }

    tracing::info!("WoW Logs dir not auto-detected — user must browse");
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn round_trips_config() {
        let dir = tempdir().unwrap();
        let mut cfg = AppConfig::default();
        cfg.intensity    = 5;
        cfg.player_focus = "Stonebraid".to_owned();
        cfg.major_cds    = vec![31884, 642];

        save(&cfg, dir.path()).unwrap();

        let loaded = load_or_default(dir.path()).unwrap();
        assert_eq!(loaded.intensity,    5);
        assert_eq!(loaded.player_focus, "Stonebraid");
        assert_eq!(loaded.major_cds,    vec![31884, 642]);
    }

    #[test]
    fn returns_default_when_missing() {
        let dir = tempdir().unwrap();
        let cfg = load_or_default(dir.path()).unwrap();
        assert_eq!(cfg.intensity, 3);
        assert!(cfg.wow_log_path.as_os_str().is_empty());
    }

    #[test]
    fn find_latest_log_picks_newest() {
        let dir = tempdir().unwrap();

        // Create two combat log files with distinct names
        let older_path = dir.path().join("WoWCombatLog_2024_01_01_100000.txt");
        let newer_path = dir.path().join("WoWCombatLog_2024_06_15_183000.txt");
        let unrelated  = dir.path().join("WoWCombatLog.txt"); // plain name also valid

        std::fs::File::create(&older_path).unwrap().write_all(b"old").unwrap();
        // Small sleep isn't reliable in CI; instead set mtime explicitly via
        // writing a byte more so the OS updates mtime for newer_path last.
        std::fs::File::create(&unrelated).unwrap().write_all(b"plain").unwrap();
        std::fs::File::create(&newer_path).unwrap().write_all(b"new").unwrap();

        let result = find_latest_log(dir.path()).unwrap();
        // The newest file by mtime should be returned (newer_path was written last)
        assert_eq!(result, newer_path);
    }

    #[test]
    fn find_latest_log_returns_none_for_empty_dir() {
        let dir = tempdir().unwrap();
        assert!(find_latest_log(dir.path()).is_none());
    }

    #[test]
    fn find_latest_log_ignores_non_combatlog_files() {
        let dir = tempdir().unwrap();
        std::fs::File::create(dir.path().join("Interface.log")).unwrap();
        std::fs::File::create(dir.path().join("addon_errors.txt")).unwrap();
        assert!(find_latest_log(dir.path()).is_none());
    }

    // -----------------------------------------------------------------------
    // WTF character scanner tests
    // -----------------------------------------------------------------------

    /// Build a fake WTF directory tree rooted under `logs_dir_parent`:
    ///   <logs_dir_parent>/WTF/Account/<account>/<realm>/<char>/
    fn build_fake_wtf(logs_parent: &std::path::Path, chars: &[(&str, &str, &str)]) {
        // chars: (account, realm, character)
        for (account, realm, character) in chars {
            let char_dir = logs_parent
                .join("WTF")
                .join("Account")
                .join(account)
                .join(realm)
                .join(character);
            std::fs::create_dir_all(&char_dir).unwrap();
        }
    }

    #[test]
    fn scan_wtf_finds_characters() {
        // Directory layout:
        //   tmpdir/
        //     Logs/                  ← the "logs_dir"
        //     WTF/Account/12345678#1/
        //       Stormrage/Stonebraid/
        //       Stormrage/Altbraid/
        //       Silvermoon/Healbraid/
        let root = tempdir().unwrap();
        let logs_dir = root.path().join("Logs");
        std::fs::create_dir_all(&logs_dir).unwrap();

        build_fake_wtf(root.path(), &[
            ("12345678#1", "Stormrage",  "Stonebraid"),
            ("12345678#1", "Stormrage",  "Altbraid"),
            ("12345678#1", "Silvermoon", "Healbraid"),
        ]);

        let chars = scan_wtf_characters(&logs_dir);
        assert_eq!(chars.len(), 3);

        // Results are sorted by realm then name
        assert_eq!(chars[0].realm, "Silvermoon");
        assert_eq!(chars[0].name,  "Healbraid");
        assert_eq!(chars[1].realm, "Stormrage");
        assert_eq!(chars[1].name,  "Altbraid");
        assert_eq!(chars[2].name,  "Stonebraid");
    }

    #[test]
    fn scan_wtf_skips_saved_variables_dir() {
        let root = tempdir().unwrap();
        let logs_dir = root.path().join("Logs");
        std::fs::create_dir_all(&logs_dir).unwrap();

        build_fake_wtf(root.path(), &[
            ("12345678#1", "Stormrage",        "Stonebraid"),
        ]);
        // Create a SavedVariables dir at the realm level (should be skipped)
        std::fs::create_dir_all(
            root.path().join("WTF").join("Account").join("12345678#1").join("SavedVariables")
        ).unwrap();

        let chars = scan_wtf_characters(&logs_dir);
        // Only the real character, not SavedVariables
        assert_eq!(chars.len(), 1);
        assert_eq!(chars[0].name, "Stonebraid");
    }

    #[test]
    fn scan_wtf_returns_empty_when_no_wtf_dir() {
        let root = tempdir().unwrap();
        let logs_dir = root.path().join("Logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        // No WTF directory at all
        assert!(scan_wtf_characters(&logs_dir).is_empty());
    }
}
