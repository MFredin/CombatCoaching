/// Fires when the coached player is hit by the same spell 2+ times in one pull.
///
/// Phase 0: fires for ANY spell that damages the player (no encounter list needed).
/// Phase 1: cross-reference against the encounter TOML avoidable_spell_ids list
///          so only truly avoidable mechanics trigger this rule.
use super::{advice, RuleContext, RuleInput, RuleOutput};
use crate::{engine::Severity, parser::LogEvent};

pub const KEY: &str = "avoidable_repeat";
const MIN_HITS: u32 = 2;

pub fn evaluate(input: &RuleInput, ctx: &RuleContext) -> RuleOutput {
    let LogEvent::SpellDamage {
        dest_guid,
        spell_id,
        spell_name,
        amount,
        ..
    } = input.event
    else {
        return vec![];
    };

    // Only fire for the coached player taking damage
    if Some(dest_guid.as_str()) != ctx.state.player_guid.as_deref() {
        return vec![];
    }

    let hit_count = ctx.state.avoidable.hit_count(*spell_id);
    if hit_count < MIN_HITS {
        return vec![];
    }

    vec![advice(
        KEY,
        "Avoidable damage repeating",
        format!(
            "{}: {} hits this pull ({} dmg last hit). Adjust position before next overlap.",
            spell_name, hit_count, amount
        ),
        Severity::Bad,
        vec![
            ("hits".to_owned(), hit_count.to_string()),
            ("spell".to_owned(), spell_name.clone()),
            ("spell_id".to_owned(), spell_id.to_string()),
        ],
        ctx.now_ms,
    )]
}
