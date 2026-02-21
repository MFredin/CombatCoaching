/// Application configuration — persisted as TOML in the platform config directory.
///
/// On Windows this is typically:
///   %APPDATA%\com.combatledger.livecoach\config.toml
///   (e.g. C:\Users\<user>\AppData\Roaming\com.combatledger.livecoach\config.toml)
///
/// First-run: if wow_log_path is empty, the settings window shows a wizard
/// that calls detect_wow_path and/or opens a file picker.
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
    /// Absolute path to WoWCombatLog.txt
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

/// Try to auto-detect the WoW combat log path from common install locations.
/// Returns the first path that exists on disk.
#[tauri::command]
pub fn detect_wow_path() -> Option<String> {
    // Common Battle.net install directories across drive letters
    let roots = ["C", "D", "E", "F", "G"];
    let suffixes = [
        r"Program Files (x86)\World of Warcraft\_retail_\Logs\WoWCombatLog.txt",
        r"Program Files (x86)\World of Warcraft\Logs\WoWCombatLog.txt",
        r"Program Files\World of Warcraft\_retail_\Logs\WoWCombatLog.txt",
        r"Program Files\World of Warcraft\Logs\WoWCombatLog.txt",
        r"World of Warcraft\_retail_\Logs\WoWCombatLog.txt",
        r"World of Warcraft\Logs\WoWCombatLog.txt",
        r"Games\World of Warcraft\_retail_\Logs\WoWCombatLog.txt",
    ];

    for root in &roots {
        for suffix in &suffixes {
            let path = PathBuf::from(format!(r"{}:\{}", root, suffix));
            if path.exists() {
                tracing::info!("Auto-detected WoW log path: {:?}", path);
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    tracing::info!("WoW log path not auto-detected — user must browse");
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
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
}
