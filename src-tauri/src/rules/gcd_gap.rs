/// Fires when the coached player has a large gap between casts (lost uptime).
///
/// The GCD tracker records the time between consecutive SPELL_CAST_SUCCESS events.
/// A gap > 2.5s suggests the player stopped pressing buttons — either from a
/// mechanic, positioning, or lost focus.
///
/// Intensity gate: only fires at intensity >= 3 (Balanced or higher).
use super::{advice, RuleContext, RuleInput, RuleOutput};
use crate::{engine::Severity, parser::LogEvent};

pub const KEY: &str = "gcd_gap";
const THRESHOLD_MS: u64 = 2_500;
const MIN_INTENSITY: u8  = 3;

pub fn evaluate(input: &RuleInput, ctx: &RuleContext) -> RuleOutput {
    // We evaluate the gap that just *ended* — i.e., after a cast completes
    let LogEvent::SpellCastSuccess { source_guid, .. } = input.event else {
        return vec![];
    };

    if Some(source_guid.as_str()) != ctx.state.player_guid.as_deref() {
        return vec![];
    }

    if ctx.intensity < MIN_INTENSITY {
        return vec![];
    }

    let gap_ms = ctx.state.gcd.current_gap_ms;
    if gap_ms < THRESHOLD_MS {
        return vec![];
    }

    let gap_s = gap_ms as f64 / 1_000.0;

    vec![advice(
        KEY,
        "Large GCD gap",
        format!(
            "You had a {:.1}s gap. Pre-position during mechanics and use a mobile filler.",
            gap_s
        ),
        Severity::Warn,
        vec![
            ("gap".to_owned(), format!("{:.1}s", gap_s)),
            ("phase".to_owned(), format!("P{}", ctx.state.pull_elapsed_ms(ctx.now_ms) / 60_000 + 1)),
        ],
        ctx.now_ms,
    )]
}
