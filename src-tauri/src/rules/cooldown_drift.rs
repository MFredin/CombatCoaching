/// Fires when a major cooldown is used significantly later than pull start.
///
/// "Drift" = time from pull start to the *first* observed use of the cooldown.
/// If you popped Avenging Wrath 15 seconds into the pull instead of on pull,
/// that's 15s of lost burst uptime.
///
/// This rule only uses observed SPELL_CAST_SUCCESS timestamps — it never claims
/// the CD "was available" without evidence. If the spell is not seen at all,
/// the rule simply does not fire.
///
/// The list of major CD spell IDs comes from the user's spec profile TOML,
/// loaded into AppConfig.major_cds at startup.
use super::{advice, RuleContext, RuleInput, RuleOutput};
use crate::{engine::Severity, parser::LogEvent};

pub const KEY: &str = "cooldown_drift";
const DRIFT_THRESHOLD_MS: u64 = 8_000; // 8 seconds

pub fn evaluate(input: &RuleInput, ctx: &RuleContext, major_cd_ids: &[u32]) -> RuleOutput {
    let LogEvent::SpellCastSuccess {
        source_guid,
        spell_id,
        spell_name,
        ..
    } = input.event
    else {
        return vec![];
    };

    if Some(source_guid.as_str()) != ctx.state.player_guid.as_deref() {
        return vec![];
    }

    if !major_cd_ids.contains(spell_id) {
        return vec![];
    }

    let pull_elapsed = ctx.state.pull_elapsed_ms(ctx.now_ms);

    // Must be past the threshold to be considered "drift"
    if pull_elapsed < DRIFT_THRESHOLD_MS {
        return vec![];
    }

    // Only report on the FIRST use of this CD this pull.
    // If the CD was used earlier, its last_used_ms will be less than pull_elapsed.
    let pull_start_ms = ctx.state.current_pull.as_ref().map(|p| p.start_ms).unwrap_or(0);
    let is_first_use = ctx
        .state
        .cooldowns
        .last_used_ms(*spell_id)
        .map(|t| t >= pull_start_ms && t == ctx.now_ms)
        .unwrap_or(false);

    if !is_first_use {
        return vec![];
    }

    let drift_s = pull_elapsed as f64 / 1_000.0;

    vec![advice(
        KEY,
        "Major cooldown used late",
        format!(
            "{} drifted by ~{:.0}s into the pull. Next pull: use on pull, then on cooldown.",
            spell_name, drift_s
        ),
        Severity::Warn,
        vec![
            ("drift".to_owned(), format!("{:.1}s", drift_s)),
            ("spell".to_owned(), spell_name.clone()),
        ],
        ctx.now_ms,
    )]
}
