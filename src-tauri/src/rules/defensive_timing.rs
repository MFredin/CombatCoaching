/// Fires Good when the coached player uses active mitigation under significant damage pressure.
///
/// "Good AM Timing" — positive feedback when the player pops a defensive
/// during a spike of incoming damage, reinforcing reactive defensive play.
///
/// Fires when:
///   - A spell in `am_ids` is cast by the coached player
///   - Damage taken in the last 5 seconds exceeds DAMAGE_THRESHOLD
///   - Intensity >= 2
///
/// The damage threshold (20,000) is a heuristic that scales reasonably
/// across Mythic+ content. No HP estimation is attempted — log-derived signals only.
use super::{advice, RuleContext, RuleInput, RuleOutput};
use crate::{engine::Severity, parser::LogEvent};

/// Minimum damage in the last 5 seconds to consider "meaningful pressure"
const DAMAGE_THRESHOLD: u64 = 20_000;
const WINDOW_MS:        u64 = 5_000;
const MIN_INTENSITY:    u8  = 2;

pub fn evaluate(input: &RuleInput, ctx: &RuleContext, am_ids: &[u32]) -> RuleOutput {
    if am_ids.is_empty() {
        return vec![];
    }

    let LogEvent::SpellCastSuccess {
        source_guid,
        spell_id,
        spell_name,
        ..
    } = input.event
    else {
        return vec![];
    };

    // Only fire for the coached player's casts
    if Some(source_guid.as_str()) != ctx.state.player_guid.as_deref() {
        return vec![];
    }

    // Only fire if this is an active mitigation spell
    if !am_ids.contains(spell_id) {
        return vec![];
    }

    if ctx.intensity < MIN_INTENSITY {
        return vec![];
    }

    let recent_dmg = ctx.state.damage_taken.recent_damage(ctx.now_ms, WINDOW_MS);
    if recent_dmg < DAMAGE_THRESHOLD {
        return vec![];
    }

    let dmg_k = recent_dmg / 1_000;

    vec![advice(
        &format!("am_under_pressure_{}", spell_id),
        "Good AM Timing",
        format!(
            "{} used under pressure — {}k damage in the last 5s.",
            spell_name, dmg_k
        ),
        Severity::Good,
        vec![
            ("spell".to_owned(),      spell_name.clone()),
            ("recent_dmg".to_owned(), format!("{}k", dmg_k)),
        ],
        ctx.now_ms,
    )]
}
