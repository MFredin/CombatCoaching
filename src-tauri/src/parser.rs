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
}

impl LogEvent {
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            Self::SpellDamage     { timestamp_ms, .. } => *timestamp_ms,
            Self::SwingDamage     { timestamp_ms, .. } => *timestamp_ms,
            Self::SpellCastSuccess{ timestamp_ms, .. } => *timestamp_ms,
            Self::SpellHeal       { timestamp_ms, .. } => *timestamp_ms,
            Self::UnitDied        { timestamp_ms, .. } => *timestamp_ms,
            Self::SpellInterrupted{ timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// GUID of the entity that performed this action, if any.
    #[allow(dead_code)] // used by engine filter in future phases
    pub fn source_guid(&self) -> Option<&str> {
        match self {
            Self::SpellDamage      { source_guid, .. } => Some(source_guid),
            Self::SwingDamage      { source_guid, .. } => Some(source_guid),
            Self::SpellCastSuccess { source_guid, .. } => Some(source_guid),
            Self::SpellHeal        { source_guid, .. } => Some(source_guid),
            Self::SpellInterrupted { source_guid, .. } => Some(source_guid),
            Self::UnitDied { .. }                      => None,
        }
    }

    /// GUID of the entity on the receiving end of this event, if any.
    #[allow(dead_code)] // used by engine filter in future phases
    pub fn dest_guid(&self) -> Option<&str> {
        match self {
            Self::SpellDamage      { dest_guid, .. } => Some(dest_guid),
            Self::SwingDamage      { dest_guid, .. } => Some(dest_guid),
            Self::SpellHeal        { dest_guid, .. } => Some(dest_guid),
            Self::UnitDied         { dest_guid, .. } => Some(dest_guid),
            Self::SpellInterrupted { target_guid, .. } => Some(target_guid),
            Self::SpellCastSuccess { .. }            => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse the WoW log timestamp prefix "M/D HH:MM:SS.mmm" into milliseconds.
/// We do not anchor to a real epoch — ms values are used only for relative
/// calculations (pull timers, gaps between events, cooldown tracking).
fn parse_timestamp(date_time: &str) -> Option<u64> {
    // date_time looks like "5/21 20:14:33.123" — split on space
    let mut parts = date_time.splitn(2, ' ');
    let _date = parts.next()?; // e.g. "5/21" — unused in Phase 0
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
///
/// WoW log lines look like:
///   "5/21 20:14:33.123  SPELL_DAMAGE,0,..."
///                      ^^  (two spaces between timestamp and payload)
fn split_line(raw: &str) -> Option<(u64, Vec<&str>)> {
    // The timestamp ends at the double-space separator
    let sep = raw.find("  ")?;
    let ts_str   = &raw[..sep];
    let payload  = &raw[sep + 2..];

    let ts_ms = parse_timestamp(ts_str)?;

    // Limit to 25 fields — enough for all event types we care about.
    // We do NOT do a full CSV parse here (Phase 0); quoted commas in names
    // are handled by unquote() stripping the surrounding quotes on known name fields.
    let fields: Vec<&str> = payload.splitn(25, ',').collect();

    Some((ts_ms, fields))
}

pub fn parse_line(raw: &str) -> Option<LogEvent> {
    let (ts, f) = split_line(raw)?;

    let src_guid  = unquote(f.get(2)?).to_owned();
    let src_name  = unquote(f.get(3)?).to_owned();
    let dst_guid  = unquote(f.get(6)?).to_owned();
    let dst_name  = unquote(f.get(7)?).to_owned();

    match *f.first()? {
        "SPELL_DAMAGE" | "SPELL_PERIODIC_DAMAGE" | "RANGE_DAMAGE" => {
            let spell_id:  u32 = f.get(10)?.parse().ok()?;
            let spell_name     = unquote(f.get(11)?).to_owned();
            // Field layout after school [12]: [13]=missType, [14]=amount, [15]=overkill, ...
            let amount:    u64 = f.get(14)?.parse().ok()?;
            Some(LogEvent::SpellDamage { timestamp_ms: ts, source_guid: src_guid, source_name: src_name, dest_guid: dst_guid, dest_name: dst_name, spell_id, spell_name, amount })
        }
        "SWING_DAMAGE" => {
            // No spell prefix — damage at field [12]
            let amount: u64 = f.get(12)?.parse().ok()?;
            Some(LogEvent::SwingDamage { timestamp_ms: ts, source_guid: src_guid, dest_guid: dst_guid, amount })
        }
        "SPELL_CAST_SUCCESS" => {
            let spell_id:  u32 = f.get(10)?.parse().ok()?;
            let spell_name     = unquote(f.get(11)?).to_owned();
            Some(LogEvent::SpellCastSuccess { timestamp_ms: ts, source_guid: src_guid, source_name: src_name, spell_id, spell_name })
        }
        "SPELL_HEAL" | "SPELL_PERIODIC_HEAL" => {
            let spell_id:    u32 = f.get(10)?.parse().ok()?;
            let amount:      u64 = f.get(14)?.parse().ok()?;
            let overhealing: u64 = f.get(15)?.parse().unwrap_or(0);
            Some(LogEvent::SpellHeal { timestamp_ms: ts, source_guid: src_guid, dest_guid: dst_guid, spell_id, amount, overhealing })
        }
        "UNIT_DIED" => {
            Some(LogEvent::UnitDied { timestamp_ms: ts, dest_guid: dst_guid, dest_name: dst_name })
        }
        "SPELL_INTERRUPT" => {
            let interrupted_spell_id: u32 = f.get(13)?.parse().ok()?;
            let interrupted_spell        = unquote(f.get(14)?).to_owned();
            Some(LogEvent::SpellInterrupted { timestamp_ms: ts, source_guid: src_guid, target_guid: dst_guid, interrupted_spell_id, interrupted_spell })
        }
        _ => None, // Unrecognised subevent — silently skip
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

    #[test]
    fn parses_spell_damage() {
        let e = parse_line(SPELL_DAMAGE_LINE).expect("should parse");
        match e {
            LogEvent::SpellDamage { spell_id, spell_name, amount, source_name, .. } => {
                assert_eq!(spell_id,   12345);
                assert_eq!(spell_name, "Shadow Surge");
                assert_eq!(amount,     55000);
                assert_eq!(source_name,"Stonebraid");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_cast_success() {
        let e = parse_line(CAST_SUCCESS_LINE).expect("should parse");
        match e {
            LogEvent::SpellCastSuccess { spell_id, spell_name, source_name, .. } => {
                assert_eq!(spell_id,   31884);
                assert_eq!(spell_name, "Avenging Wrath");
                assert_eq!(source_name,"Stonebraid");
            }
            other => panic!("Wrong variant: {:?}", other),
        }
    }

    #[test]
    fn parses_unit_died() {
        let e = parse_line(UNIT_DIED_LINE).expect("should parse");
        match e {
            LogEvent::UnitDied { dest_name, .. } => {
                assert_eq!(dest_name, "Boss");
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
