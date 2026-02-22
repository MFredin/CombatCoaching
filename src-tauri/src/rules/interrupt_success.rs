/// Fires Good when the coached player successfully interrupts an enemy cast.
///
/// Positive reinforcement — let the player know their kick landed.
/// Uses a per-spell dedup key so repeated interrupts of the same spell
/// don't spam the feed, but each distinct spell gets acknowledged.
///
/// Intensity gate: fires at intensity >= 2 (Low or higher).
use super::{advice, RuleContext, RuleInput, RuleOutput};
use crate::{engine::Severity, parser::LogEvent};

const MIN_INTENSITY: u8 = 2;

pub fn evaluate(input: &RuleInput, ctx: &RuleContext) -> RuleOutput {
    let LogEvent::SpellInterrupted {
        source_guid,
        interrupted_spell_id,
        interrupted_spell,
        ..
    } = input.event
    else {
        return vec![];
    };

    // Only fire for the coached player's interrupts
    if Some(source_guid.as_str()) != ctx.state.player_guid.as_deref() {
        return vec![];
    }

    if ctx.intensity < MIN_INTENSITY {
        return vec![];
    }

    vec![advice(
        &format!("interrupt_success_{}", interrupted_spell_id),
        "Interrupt!",
        format!("Good kick — {} stopped.", interrupted_spell),
        Severity::Good,
        vec![
            ("spell".to_owned(), interrupted_spell.clone()),
            ("id".to_owned(),    interrupted_spell_id.to_string()),
        ],
        ctx.now_ms,
    )]
}
