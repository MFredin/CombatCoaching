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

// ---------------------------------------------------------------------------
// AppConfig
// ---------------------------------------------------------------------------

/// Position and visibility of a single overlay panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelPosition {
    /// Panel identifier — matches the `data-panel-id` attribute in the overlay.
    /// Known IDs: "now_feed", "pull_clock", "timeline", "stat_widgets"
    pub id:      String,
    pub x:       i32,
    pub y:       i32,
    pub visible: bool,
}

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
}

fn default_intensity() -> u8 { 3 }

fn default_panel_positions() -> Vec<PanelPosition> {
    vec![
        PanelPosition { id: "pull_clock".to_owned(),   x: 20,  y: 20,  visible: true },
        PanelPosition { id: "now_feed".to_owned(),     x: 20,  y: 70,  visible: true },
        PanelPosition { id: "timeline".to_owned(),     x: 20,  y: 500, visible: true },
        PanelPosition { id: "stat_widgets".to_owned(), x: 20,  y: 670, visible: true },
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
}
