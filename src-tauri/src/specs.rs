/// Spec profile library — embedded at compile time from `data/specs/*.toml`.
///
/// Profiles provide the major CD and active mitigation spell IDs used by the
/// cooldown_drift and defensive_timing coaching rules.  Embedding the files
/// at compile time means no runtime path resolution is needed.
///
/// The engine auto-loads a profile when the addon sends an identity update.
/// Users can also explicitly select a spec in the settings UI, which saves
/// the major CD IDs to `AppConfig.major_cds` for persistence.
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Embedded TOML data
// ---------------------------------------------------------------------------

const PALADIN_RETRIBUTION: &str = include_str!("../../data/specs/paladin_retribution.toml");
const PRIEST_HOLY:         &str = include_str!("../../data/specs/priest_holy.toml");
const WARRIOR_PROTECTION:  &str = include_str!("../../data/specs/warrior_protection.toml");

static ALL_SPEC_DATA: &[&str] = &[
    PALADIN_RETRIBUTION,
    PRIEST_HOLY,
    WARRIOR_PROTECTION,
];

// ---------------------------------------------------------------------------
// TOML deserialization structs (private)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TomlFile {
    spec: TomlSpecMeta,
}

#[derive(Deserialize)]
struct TomlSpecMeta {
    class:             String,
    spec:              String,
    role:              String,
    #[serde(default)]
    description:       String,
    cooldowns:         TomlCooldowns,
    active_mitigation: Option<TomlActiveMitigation>,
    #[allow(dead_code)]
    rotation:          Option<TomlRotation>,
}

#[derive(Deserialize)]
struct TomlCooldowns {
    major_cd_spell_ids: Vec<u32>,
}

#[derive(Deserialize)]
struct TomlActiveMitigation {
    am_spell_ids: Vec<u32>,
}

#[derive(Deserialize)]
struct TomlRotation {
    #[allow(dead_code)]
    primary_spell_ids: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A fully-parsed spec profile used by the engine.
#[derive(Debug, Clone)]
pub struct SpecProfile {
    pub class:              String,
    pub spec_name:          String,
    pub role:               String,
    /// Spell IDs of major cooldowns for the `cooldown_drift` rule.
    pub major_cd_spell_ids: Vec<u32>,
    /// Spell IDs of active mitigation / defensive abilities for future rules.
    pub am_spell_ids:       Vec<u32>,
}

impl SpecProfile {
    /// Canonical "CLASS/Spec" key used for config storage and display.
    pub fn key(&self) -> String {
        format!("{}/{}", self.class, self.spec_name)
    }
}

/// Lightweight spec descriptor returned to the frontend for dropdowns.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpecInfo {
    /// "CLASS/Spec" key — used as the value for `AppConfig.selected_spec`
    pub key:   String,
    pub class: String,
    pub spec:  String,
    pub role:  String,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn parse_all() -> Vec<SpecProfile> {
    ALL_SPEC_DATA
        .iter()
        .filter_map(|toml_str| {
            let file: TomlFile = toml::from_str(toml_str)
                .map_err(|e| tracing::warn!("Failed to parse spec TOML: {}", e))
                .ok()?;
            Some(SpecProfile {
                class:              file.spec.class,
                spec_name:          file.spec.spec,
                role:               file.spec.role,
                major_cd_spell_ids: file.spec.cooldowns.major_cd_spell_ids,
                am_spell_ids:       file.spec.active_mitigation
                                        .map(|am| am.am_spell_ids)
                                        .unwrap_or_default(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return display info for all embedded spec profiles (for the settings UI).
pub fn list_all() -> Vec<SpecInfo> {
    parse_all()
        .into_iter()
        .map(|p| SpecInfo {
            key:   p.key(),
            class: p.class,
            spec:  p.spec_name,
            role:  p.role,
        })
        .collect()
}

/// Load a spec profile by class and spec name (case-insensitive).
///
/// Returns `None` if no embedded profile matches.
pub fn load_spec(class: &str, spec_name: &str) -> Option<SpecProfile> {
    parse_all().into_iter().find(|p| {
        p.class.eq_ignore_ascii_case(class) && p.spec_name.eq_ignore_ascii_case(spec_name)
    })
}

/// Load a spec profile by its canonical "CLASS/Spec" key.
pub fn load_by_key(key: &str) -> Option<SpecProfile> {
    let (class, spec) = key.split_once('/')?;
    load_spec(class, spec)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_three_specs() {
        let specs = list_all();
        assert_eq!(specs.len(), 3);
        let keys: Vec<&str> = specs.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"PALADIN/Retribution"));
        assert!(keys.contains(&"PRIEST/Holy"));
        assert!(keys.contains(&"WARRIOR/Protection"));
    }

    #[test]
    fn loads_paladin_ret() {
        let p = load_spec("PALADIN", "Retribution").expect("should load");
        assert!(!p.major_cd_spell_ids.is_empty());
        assert!(p.major_cd_spell_ids.contains(&31884)); // Avenging Wrath
        assert!(p.am_spell_ids.contains(&498));          // Divine Protection
    }

    #[test]
    fn loads_by_key() {
        let p = load_by_key("WARRIOR/Protection").expect("should load");
        assert!(p.major_cd_spell_ids.contains(&871)); // Shield Wall
    }

    #[test]
    fn case_insensitive_lookup() {
        assert!(load_spec("paladin", "retribution").is_some());
        assert!(load_by_key("warrior/protection").is_some());
    }

    #[test]
    fn returns_none_for_unknown() {
        assert!(load_spec("MAGE", "Fire").is_none());
    }

    #[test]
    fn key_format() {
        let p = load_spec("PALADIN", "Retribution").unwrap();
        assert_eq!(p.key(), "PALADIN/Retribution");
    }
}
