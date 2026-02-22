/// Parses raw WoW combat log lines into typed `LogEvent` structs.
///
/// WoW combat log format (The War Within / Midnight, 11.x+):
///
///   TIMESTAMP  SUBEVENT,HIDECASTER,SOURCEGUID,SOURCENAME,SOURCEFLAGS,SOURCERAIDFLAGS,
///              DESTGUID,DESTNAME,DESTFLAGS,DESTROAIDFLAGS,[subevent-specific fields...]
///
/// Field indices (0-based after splitting on comma):
///   [0]  subevent name (e.g. "SPELL_DAMAGE")
///   [1]  hidecaster (0 or 1)
///   [2]  source GUID
///   [3]  source name (quoted)
///   [4]  source flags
///   [5]  source raid flags
///   [6]  dest GUID
///   [7]  dest name (quoted)
///   [8]  dest flags
///   [9]  dest raid flags
///   [10] spell ID       (prefix fields for SPELL_* events)
///   [11] spell name     (quoted)
///   [12] spell school
///   [13+] subevent-specific
///
/// Note: SWING_* events have no spell prefix — their damage fields start at [10].
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Receiver, Sender};

/// Typed combat log events the coaching engine cares about.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LogEvent {
    SpellDamage {
        timestamp_ms: u64,
        source_guid:  String,
        source_name:  String,
        dest_guid:    String,
        dest_name:    String,
        spell_id:     u32,
        spell_name:   String,
        amount:       u64,
    },
    SwingDamage {
        timestamp_ms: u64,
        source_guid:  String,
        dest_guid:    String,
        amount:       u64,
    },
    SpellCastSuccess {
        timestamp_ms: u64,
        source_guid:  String,
        source_name:  String,
        spell_id:     u32,
        spell_name:   String,
    },
    SpellHeal {
        timestamp_ms: u64,
        source_guid:  String,
        dest_guid:    String,
        spell_id:     u32,
        amount:       u64,
        overhealing:  u64,
    },
    UnitDied {
        timestamp_ms: u64,
        dest_guid:    String,
        dest_name:    String,
    },
    SpellInterrupted {
        timestamp_ms:         u64,
        source_guid:          String,
        target_guid:          String,
        interrupted_spell_id: u32,
        interrupted_spell:    String,
    },
    // ── v0.8.7 additions ──────────────────────────────────────────────────────
    /// ENCOUNTER_START — authoritative pull start with encounter metadata.
    EncounterStart {
        timestamp_ms:  u64,
        encounter_id:  u32,
        encounter_name: String,
        difficulty_id: u32,
        group_size:    u32,
    },
    /// ENCOUNTER_END — authoritative pull end with success flag.
    EncounterEnd {
        timestamp_ms:  u64,
        encounter_id:  u32,
        encounter_name: String,
        success:       bool,
    },
    /// SPELL_CAST_FAILED — player cast interrupted by movement/silence/etc.
    SpellCastFailed {
        timestamp_ms: u64,
        source_guid:  String,
        source_name:  String,
        spell_id:     u32,
        spell_name:   String,
        failed_type:  String,
    },
    /// SPELL_CAST_START — enemy or player begins casting (for interrupt timing).
    SpellCastStart {
        timestamp_ms: u64,
        source_guid:  String,
        source_name:  String,
        spell_id:     u32,
        spell_name:   String,
    },
}

impl LogEvent {
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            Self::SpellDamage      { timestamp_ms, .. } => *timestamp_ms,
            Self::SwingDamage      { timestamp_ms, .. } => *timestamp_ms,
            Self::SpellCastSuccess { timestamp_ms, .. } => *timestamp_ms,
            Self::SpellHeal        { timestamp_ms, .. } => *timestamp_ms,
            Self::UnitDied         { timestamp_ms, .. } => *timestamp_ms,
            Self::SpellInterrupted { timestamp_ms, .. } => *timestamp_ms,
            Self::EncounterStart   { timestamp_ms, .. } => *timestamp_ms,
            Self::EncounterEnd     { timestamp_ms, .. } => *timestamp_ms,
            Self::SpellCastFailed  { timestamp_ms, .. } => *timestamp_ms,
            Self::SpellCastStart   { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// GUID of the entity that performed this action, if any.
    #[allow(dead_code)]
    pub fn source_guid(&self) -> Option<&str> {
        match self {
            Self::SpellDamage      { source_guid, .. } => Some(source_guid),
            Self::SwingDamage      { source_guid, .. } => Some(source_guid),
            Self::SpellCastSuccess { source_guid, .. } => Some(source_guid),
            Self::SpellHeal        { source_guid, .. } => Some(source_guid),
            Self::SpellInterrupted { source_guid, .. } => Some(source_guid),
            Self::SpellCastFailed  { source_guid, .. } => Some(source_guid),
            Self::SpellCastStart   { source_guid, .. } => Some(source_guid),
            Self::UnitDied { .. }
            | Self::EncounterStart { .. }
            | Self::EncounterEnd { .. }              => None,
        }
    }

    /// GUID of the entity on the receiving end of this event, if any.
    #[allow(dead_code)]
    pub fn dest_guid(&self) -> Option<&str> {
        match self {
            Self::SpellDamage      { dest_guid, .. }   => Some(dest_guid),
            Self::SwingDamage      { dest_guid, .. }   => Some(dest_guid),
            Self::SpellHeal        { dest_guid, .. }   => Some(dest_guid),
            Self::UnitDied         { dest_guid, .. }   => Some(dest_guid),
            Self::SpellInterrupted { target_guid, .. } => Some(target_guid),
            Self::SpellCastSuccess { .. }
            | Self::SpellCastFailed { .. }
            | Self::SpellCastStart { .. }
            | Self::EncounterStart { .. }
            | Self::EncounterEnd { .. }                => None,
        }
    }
}

// ---------------------------------------------------------------------------
// CSV field splitter (Phase 1 — handles quoted commas in NPC names)
// ---------------------------------------------------------------------------

/// Split a CSV payload into fields, respecting double-quoted fields.
///
/// WoW log fields are either plain values or `"quoted strings"`.
/// Quoted fields may contain commas (rare but possible in NPC names).
/// The surrounding `"` are preserved in the returned slice so `unquote()`
/// can still strip them on known name fields.
fn csv_fields(s: &str, max: usize) -> Vec<&str> {
    let mut fields = Vec::with_capacity(max.min(30));
    let mut rest = s;

    while fields.len() < max {
        if rest.is_empty() {
            break;
        }
        if rest.starts_with('"') {
            // Quoted field: find the closing '"'
            let inner = &rest[1..];
            let close = inner.find('"').unwrap_or(inner.len());
            // Include both surrounding quotes in the slice
            let field_end = close + 2; // +2 for the two '"'
            let field_end = field_end.min(rest.len());
            fields.push(&rest[..field_end]);
            let after = &rest[field_end..];
            rest = if after.starts_with(',') { &after[1..] } else { after };
        } else {
            // Unquoted field: scan to next comma
            let end = rest.find(',').unwrap_or(rest.len());
            fields.push(&rest[..end]);
            rest = if end < rest.len() { &rest[end + 1..] } else { "" };
        }
    }

    fields
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse the WoW log timestamp prefix "M/D HH:MM:SS.mmm" into milliseconds.
fn parse_timestamp(date_time: &str) -> Option<u64> {
    let mut parts = date_time.splitn(2, ' ');
    let _date = parts.next()?; // e.g. "5/21" — unused (Phase 0)
    let time  = parts.next()?; // e.g. "20:14:33.123"

    let mut time_parts = time.splitn(3, ':');
    let h:  u64 = time_parts.next()?.parse().ok()?;
    let m:  u64 = time_parts.next()?.parse().ok()?;
    let sm: &str = time_parts.next()?;

    let (s_str, ms_str) = sm.split_once('.').unwrap_or((sm, "0"));
    let s:  u64 = s_str.parse().ok()?;
    let ms: u64 = ms_str.parse().ok()?;

    Some((h * 3_600 + m * 60 + s) * 1_000 + ms)
}

/// Strip surrounding double-quotes from a field value.
#[inline]
fn unquote(s: &str) -> &str {
    s.trim_matches('"')
}

/// Split a raw log line into (timestamp_ms, fields[]).
fn split_line(raw: &str) -> Option<(u64, Vec<&str>)> {
    // The timestamp ends at the double-space separator
    let sep     = raw.find("  ")?;
    let ts_str  = &raw[..sep];
    let payload = &raw[sep + 2..];

    let ts_ms = parse_timestamp(ts_str)?;
    let fields = csv_fields(payload, 30);

    Some((ts_ms, fields))
}

pub fn parse_line(raw: &str) -> Option<LogEvent> {
    let (ts, f) = split_line(raw)?;

    let src_guid = unquote(f.get(2)?).to_owned();
    let src_name = unquote(f.get(3)?).to_owned();
    // ENCOUNTER_START / ENCOUNTER_END have only 5 fields and no source/dest
    // header, so f[6] and f[7] don't exist.  Use map_or so those events can
    // still reach their match arm instead of returning None here.
    let dst_guid = f.get(6).map_or("", |s| unquote(s)).to_owned();
    let dst_name = f.get(7).map_or("", |s| unquote(s)).to_owned();

    match *f.first()? {
        "SPELL_DAMAGE" | "SPELL_PERIODIC_DAMAGE" | "RANGE_DAMAGE" => {
            let spell_id:  u32 = f.get(10)?.parse().ok()?;
            let spell_name     = unquote(f.get(11)?).to_owned();
            let amount:    u64 = f.get(14)?.parse().ok()?;
            Some(LogEvent::SpellDamage {
                timestamp_ms: ts, source_guid: src_guid, source_name: src_name,
                dest_guid: dst_guid, dest_name: dst_name, spell_id, spell_name, amount,
            })
        }
        "SWING_DAMAGE" => {
            let amount: u64 = f.get(12)?.parse().ok()?;
            Some(LogEvent::SwingDamage {
                timestamp_ms: ts, source_guid: src_guid, dest_guid: dst_guid, amount,
            })
        }
        "SPELL_CAST_SUCCESS" => {
            let spell_id:  u32 = f.get(10)?.parse().ok()?;
            let spell_name     = unquote(f.get(11)?).to_owned();
            Some(LogEvent::SpellCastSuccess {
                timestamp_ms: ts, source_guid: src_guid, source_name: src_name,
                spell_id, spell_name,
            })
        }
        "SPELL_HEAL" | "SPELL_PERIODIC_HEAL" => {
            let spell_id:    u32 = f.get(10)?.parse().ok()?;
            let amount:      u64 = f.get(14)?.parse().ok()?;
            let overhealing: u64 = f.get(15)?.parse().unwrap_or(0);
            Some(LogEvent::SpellHeal {
                timestamp_ms: ts, source_guid: src_guid, dest_guid: dst_guid,
                spell_id, amount, overhealing,
            })
        }
        "UNIT_DIED" => {
            Some(LogEvent::UnitDied {
                timestamp_ms: ts, dest_guid: dst_guid, dest_name: dst_name,
            })
        }
        "SPELL_INTERRUPT" => {
            let interrupted_spell_id: u32 = f.get(13)?.parse().ok()?;
            let interrupted_spell        = unquote(f.get(14)?).to_owned();
            Some(LogEvent::SpellInterrupted {
                timestamp_ms: ts, source_guid: src_guid, target_guid: dst_guid,
                interrupted_spell_id, interrupted_spell,
            })
        }
        // ── v0.8.7 additions ──────────────────────────────────────────────
        "ENCOUNTER_START" => {
            // ENCOUNTER_START,encounter_id,"Encounter Name",difficulty_id,group_size
            // These 5 fields replace the standard 10-field header entirely.
            let encounter_id:  u32 = f.get(1)?.parse().ok()?;
            let encounter_name     = unquote(f.get(2)?).to_owned();
            let difficulty_id: u32 = f.get(3)?.parse().unwrap_or(0);
            let group_size:    u32 = f.get(4)?.parse().unwrap_or(0);
            Some(LogEvent::EncounterStart {
                timestamp_ms: ts, encounter_id, encounter_name, difficulty_id, group_size,
            })
        }
        "ENCOUNTER_END" => {
            // ENCOUNTER_END,encounter_id,"Encounter Name",difficulty_id,group_size,success
            let encounter_id:  u32 = f.get(1)?.parse().ok()?;
            let encounter_name     = unquote(f.get(2)?).to_owned();
            // success: 1 = win, 0 = wipe
            let success: bool = f.get(5)
                .and_then(|s| s.parse::<u8>().ok())
                .map(|v| v == 1)
                .unwrap_or(false);
            Some(LogEvent::EncounterEnd {
                timestamp_ms: ts, encounter_id, encounter_name, success,
            })
        }
        "SPELL_CAST_FAILED" => {
            let spell_id:  u32 = f.get(10)?.parse().ok()?;
            let spell_name     = unquote(f.get(11)?).to_owned();
            let failed_type    = unquote(f.get(13).unwrap_or(&"")).to_owned();
            Some(LogEvent::SpellCastFailed {
                timestamp_ms: ts, source_guid: src_guid, source_name: src_name,
                spell_id, spell_name, failed_type,
            })
        }
        "SPELL_CAST_START" => {
            let spell_id:  u32 = f.get(10)?.parse().ok()?;
            let spell_name     = unquote(f.get(11)?).to_owned();
            Some(LogEvent::SpellCastStart {
                timestamp_ms: ts, source_guid: src_guid, source_name: src_name,
                spell_id, spell_name,
            })
        }
        _ => None,
    }
}

/// Async pipeline task: receive raw lines, parse, forward typed events.
pub async fn run(mut rx: Receiver<String>, tx: Sender<LogEvent>) -> Result<()> {
    while let Some(line) = rx.recv().await {
        if let Some(event) = parse_line(&line) {
            if tx.send(event).await.is_err() {
                break;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    const SPELL_DAMAGE_LINE: &str =
        r#"5/21 20:14:33.456  SPELL_DAMAGE,0,Player-1234-ABCDEF,"Stonebraid",0x511,0x0,Creature-0-4372-ABCD-000,"Boss",0xa48,0x0,12345,"Shadow Surge",0x20,0,55000,0,0,0,0,nil,nil,nil"#;

    const CAST_SUCCESS_LINE: &str =
        r#"5/21 20:14:35.100  SPELL_CAST_SUCCESS,0,Player-1234-ABCDEF,"Stonebraid",0x511,0x0,0000000000000000,"",0x80,0x0,31884,"Avenging Wrath",0x2"#;

    const UNIT_DIED_LINE: &str =
        r#"5/21 20:15:00.000  UNIT_DIED,0,0000000000000000,"",0x80,0x0,Creature-0-4372-ABCD-000,"Boss",0xa48,0x0,0"#;

    const ENCOUNTER_START_LINE: &str =
        r#"5/21 20:14:30.000  ENCOUNTER_START,2920,"The Necrotic Wake",14,5"#;

    const ENCOUNTER_END_WIN_LINE: &str =
        r#"5/21 20:20:00.000  ENCOUNTER_END,2920,"The Necrotic Wake",14,5,1"#;

    const ENCOUNTER_END_WIPE_LINE: &str =
        r#"5/21 20:18:00.000  ENCOUNTER_END,2920,"The Necrotic Wake",14,5,0"#;

    const CAST_FAILED_LINE: &str =
        r#"5/21 20:14:34.200  SPELL_CAST_FAILED,0,Player-1234-ABCDEF,"Stonebraid",0x511,0x0,0000000000000000,"",0x80,0x0,31884,"Avenging Wrath",0x2,MOVING"#;

    const CAST_START_LINE: &str =
        r#"5/21 20:14:34.000  SPELL_CAST_START,0,Creature-0-4372-ABCD-000,"Boss",0xa48,0x0,0000000000000000,"",0x80,0x0,99999,"Void Bolt",0x40"#;

    const QUOTED_COMMA_LINE: &str =
        r#"5/21 20:14:33.456  SPELL_DAMAGE,0,Creature-0-1234-ABCD-000,"Kel'Thuzad, the Undying",0xa48,0x0,Player-1234-ABCDEF,"Stonebraid",0x511,0x0,12345,"Frost Bolt",0x10,0,30000,0,0,0,0,nil,nil,nil"#;

    #[test]
    fn parses_spell_damage() {
        let e = parse_line(SPELL_DAMAGE_LINE).expect("should parse");
        match e {
            LogEvent::SpellDamage { spell_id, spell_name, amount, source_name, .. } => {
                assert_eq!(spell_id,    12345);
                assert_eq!(spell_name, "Shadow Surge");
                assert_eq!(amount,      55000);
                assert_eq!(source_name, "Stonebraid");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_cast_success() {
        let e = parse_line(CAST_SUCCESS_LINE).expect("should parse");
        match e {
            LogEvent::SpellCastSuccess { spell_id, spell_name, source_name, .. } => {
                assert_eq!(spell_id,    31884);
                assert_eq!(spell_name, "Avenging Wrath");
                assert_eq!(source_name, "Stonebraid");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_unit_died() {
        let e = parse_line(UNIT_DIED_LINE).expect("should parse");
        match e {
            LogEvent::UnitDied { dest_name, .. } => assert_eq!(dest_name, "Boss"),
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_encounter_start() {
        let e = parse_line(ENCOUNTER_START_LINE).expect("should parse");
        match e {
            LogEvent::EncounterStart { encounter_id, encounter_name, group_size, .. } => {
                assert_eq!(encounter_id,   2920);
                assert_eq!(encounter_name, "The Necrotic Wake");
                assert_eq!(group_size,     5);
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_encounter_end_win() {
        let e = parse_line(ENCOUNTER_END_WIN_LINE).expect("should parse");
        match e {
            LogEvent::EncounterEnd { success, encounter_name, .. } => {
                assert!(success);
                assert_eq!(encounter_name, "The Necrotic Wake");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_encounter_end_wipe() {
        let e = parse_line(ENCOUNTER_END_WIPE_LINE).expect("should parse");
        match e {
            LogEvent::EncounterEnd { success, .. } => assert!(!success),
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_spell_cast_failed() {
        let e = parse_line(CAST_FAILED_LINE).expect("should parse");
        match e {
            LogEvent::SpellCastFailed { spell_id, failed_type, .. } => {
                assert_eq!(spell_id,    31884);
                assert_eq!(failed_type, "MOVING");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_spell_cast_start() {
        let e = parse_line(CAST_START_LINE).expect("should parse");
        match e {
            LogEvent::SpellCastStart { spell_id, spell_name, source_name, .. } => {
                assert_eq!(spell_id,    99999);
                assert_eq!(spell_name, "Void Bolt");
                assert_eq!(source_name, "Boss");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn handles_quoted_comma_in_npc_name() {
        // "Kel'Thuzad, the Undying" has a comma inside the quotes — dest is the
        // player "Stonebraid" and should land at field index 7.
        let e = parse_line(QUOTED_COMMA_LINE).expect("should parse");
        match e {
            LogEvent::SpellDamage { dest_name, source_name, spell_name, .. } => {
                assert_eq!(dest_name,   "Stonebraid");
                assert_eq!(source_name, "Kel'Thuzad, the Undying");
                assert_eq!(spell_name,  "Frost Bolt");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn returns_none_for_garbage() {
        assert!(parse_line("not a log line").is_none());
        assert!(parse_line("").is_none());
    }
}
