/// Fires Bad when an enemy spell completes that the player has previously interrupted.
///
/// "You know how to stop this — you missed the kick on [Spell]."
///
/// The interrupt tracker learns which spells are interruptible from observed
/// SpellInterrupted events (built up over the session). This rule only fires
/// when we have direct evidence the player CAN and HAS kicked this spell before.
///
/// Intensity gate: fires at intensity >= 3 (Balanced or higher).
use super::{advice, RuleContext, RuleInput, RuleOutput};
use crate::{engine::Severity, parser::LogEvent};

const MIN_INTENSITY: u8 = 3;

pub fn evaluate(input: &RuleInput, ctx: &RuleContext) -> RuleOutput {
    // We care about enemy SPELL_CAST_SUCCESS for spells we know are interruptible
    let LogEvent::SpellCastSuccess {
        source_guid,
        spell_id,
        spell_name,
        ..
    } = input.event
    else {
        return vec![];
    };

    // Skip the coached player's own casts
    if Some(source_guid.as_str()) == ctx.state.player_guid.as_deref() {
        return vec![];
    }

    // Only fire for creature/vehicle (enemy) casts, not party members
    if !source_guid.starts_with("Creature") && !source_guid.starts_with("Vehicle") {
        return vec![];
    }

    // Only fire if we have previously seen this spell interrupted
    if !ctx.state.interrupts.is_interruptible(*spell_id) {
        return vec![];
    }

    // Only fire while in combat
    if !ctx.state.in_combat {
        return vec![];
    }

    if ctx.intensity < MIN_INTENSITY {
        return vec![];
    }

    vec![advice(
        &format!("interrupt_miss_{}", spell_id),
        "Missed Interrupt",
        format!("{} went through — you can kick this.", spell_name),
        Severity::Bad,
        vec![
            ("spell".to_owned(),    spell_name.clone()),
            ("spell_id".to_owned(), spell_id.to_string()),
        ],
        ctx.now_ms,
    )]
}
