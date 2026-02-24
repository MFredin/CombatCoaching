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
// Embedded TOML data — one const per spec, alphabetical by file name
// ---------------------------------------------------------------------------

const DEATH_KNIGHT_BLOOD:        &str = include_str!("../../data/specs/death_knight_blood.toml");
const DEATH_KNIGHT_FROST:        &str = include_str!("../../data/specs/death_knight_frost.toml");
const DEATH_KNIGHT_UNHOLY:       &str = include_str!("../../data/specs/death_knight_unholy.toml");
const DEMON_HUNTER_HAVOC:        &str = include_str!("../../data/specs/demon_hunter_havoc.toml");
const DEMON_HUNTER_VENGEANCE:    &str = include_str!("../../data/specs/demon_hunter_vengeance.toml");
const DRUID_BALANCE:             &str = include_str!("../../data/specs/druid_balance.toml");
const DRUID_FERAL:               &str = include_str!("../../data/specs/druid_feral.toml");
const DRUID_GUARDIAN:            &str = include_str!("../../data/specs/druid_guardian.toml");
const DRUID_RESTORATION:         &str = include_str!("../../data/specs/druid_restoration.toml");
const EVOKER_AUGMENTATION:       &str = include_str!("../../data/specs/evoker_augmentation.toml");
const EVOKER_DEVASTATION:        &str = include_str!("../../data/specs/evoker_devastation.toml");
const EVOKER_PRESERVATION:       &str = include_str!("../../data/specs/evoker_preservation.toml");
const HUNTER_BEAST_MASTERY:      &str = include_str!("../../data/specs/hunter_beast_mastery.toml");
const HUNTER_MARKSMANSHIP:       &str = include_str!("../../data/specs/hunter_marksmanship.toml");
const HUNTER_SURVIVAL:           &str = include_str!("../../data/specs/hunter_survival.toml");
const MAGE_ARCANE:               &str = include_str!("../../data/specs/mage_arcane.toml");
const MAGE_FIRE:                 &str = include_str!("../../data/specs/mage_fire.toml");
const MAGE_FROST:                &str = include_str!("../../data/specs/mage_frost.toml");
const MONK_BREWMASTER:           &str = include_str!("../../data/specs/monk_brewmaster.toml");
const MONK_MISTWEAVER:           &str = include_str!("../../data/specs/monk_mistweaver.toml");
const MONK_WINDWALKER:           &str = include_str!("../../data/specs/monk_windwalker.toml");
const PALADIN_HOLY:              &str = include_str!("../../data/specs/paladin_holy.toml");
const PALADIN_PROTECTION:        &str = include_str!("../../data/specs/paladin_protection.toml");
const PALADIN_RETRIBUTION:       &str = include_str!("../../data/specs/paladin_retribution.toml");
const PRIEST_DISCIPLINE:         &str = include_str!("../../data/specs/priest_discipline.toml");
const PRIEST_HOLY:               &str = include_str!("../../data/specs/priest_holy.toml");
const PRIEST_SHADOW:             &str = include_str!("../../data/specs/priest_shadow.toml");
const ROGUE_ASSASSINATION:       &str = include_str!("../../data/specs/rogue_assassination.toml");
const ROGUE_OUTLAW:              &str = include_str!("../../data/specs/rogue_outlaw.toml");
const ROGUE_SUBTLETY:            &str = include_str!("../../data/specs/rogue_subtlety.toml");
const SHAMAN_ELEMENTAL:          &str = include_str!("../../data/specs/shaman_elemental.toml");
const SHAMAN_ENHANCEMENT:        &str = include_str!("../../data/specs/shaman_enhancement.toml");
const SHAMAN_RESTORATION:        &str = include_str!("../../data/specs/shaman_restoration.toml");
const WARLOCK_AFFLICTION:        &str = include_str!("../../data/specs/warlock_affliction.toml");
const WARLOCK_DEMONOLOGY:        &str = include_str!("../../data/specs/warlock_demonology.toml");
const WARLOCK_DESTRUCTION:       &str = include_str!("../../data/specs/warlock_destruction.toml");
const WARRIOR_ARMS:              &str = include_str!("../../data/specs/warrior_arms.toml");
const WARRIOR_FURY:              &str = include_str!("../../data/specs/warrior_fury.toml");
const WARRIOR_PROTECTION:        &str = include_str!("../../data/specs/warrior_protection.toml");

static ALL_SPEC_DATA: &[&str] = &[
    DEATH_KNIGHT_BLOOD,
    DEATH_KNIGHT_FROST,
    DEATH_KNIGHT_UNHOLY,
    DEMON_HUNTER_HAVOC,
    DEMON_HUNTER_VENGEANCE,
    DRUID_BALANCE,
    DRUID_FERAL,
    DRUID_GUARDIAN,
    DRUID_RESTORATION,
    EVOKER_AUGMENTATION,
    EVOKER_DEVASTATION,
    EVOKER_PRESERVATION,
    HUNTER_BEAST_MASTERY,
    HUNTER_MARKSMANSHIP,
    HUNTER_SURVIVAL,
    MAGE_ARCANE,
    MAGE_FIRE,
    MAGE_FROST,
    MONK_BREWMASTER,
    MONK_MISTWEAVER,
    MONK_WINDWALKER,
    PALADIN_HOLY,
    PALADIN_PROTECTION,
    PALADIN_RETRIBUTION,
    PRIEST_DISCIPLINE,
    PRIEST_HOLY,
    PRIEST_SHADOW,
    ROGUE_ASSASSINATION,
    ROGUE_OUTLAW,
    ROGUE_SUBTLETY,
    SHAMAN_ELEMENTAL,
    SHAMAN_ENHANCEMENT,
    SHAMAN_RESTORATION,
    WARLOCK_AFFLICTION,
    WARLOCK_DEMONOLOGY,
    WARLOCK_DESTRUCTION,
    WARRIOR_ARMS,
    WARRIOR_FURY,
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
    #[allow(dead_code)]
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
    fn lists_all_specs() {
        let specs = list_all();
        // 13 WoW classes × 3 specs each, except Demon Hunter (2) = 39 total
        assert_eq!(specs.len(), 39);
        // Spot-check a few across different classes
        let keys: Vec<&str> = specs.iter().map(|s| s.key.as_str()).collect();
        assert!(keys.contains(&"PALADIN/Retribution"));
        assert!(keys.contains(&"PRIEST/Holy"));
        assert!(keys.contains(&"WARRIOR/Protection"));
        assert!(keys.contains(&"MAGE/Fire"));
        assert!(keys.contains(&"DEATH_KNIGHT/Blood"));
        assert!(keys.contains(&"HUNTER/Beast Mastery"));
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
        // "TINKER" is not a WoW class — no spec file will match
        assert!(load_spec("TINKER", "Mechagnome").is_none());
    }

    #[test]
    fn key_format() {
        let p = load_spec("PALADIN", "Retribution").unwrap();
        assert_eq!(p.key(), "PALADIN/Retribution");
    }
}
