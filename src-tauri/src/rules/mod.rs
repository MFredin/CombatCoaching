pub mod avoidable_repeat;
pub mod cooldown_drift;
pub mod gcd_gap;

use crate::{
    engine::{AdviceEvent, Severity},
    identity::PlayerIdentity,
    parser::LogEvent,
    state::CombatState,
};

/// Read-only context passed to every rule evaluator.
pub struct RuleContext<'a> {
    pub state:    &'a CombatState,
    pub identity: &'a PlayerIdentity,
    /// Coaching intensity from user settings (1 = quiet, 5 = aggressive)
    pub intensity: u8,
    pub now_ms:   u64,
}

/// The current event being evaluated.
pub struct RuleInput<'a> {
    pub event: &'a LogEvent,
}

/// Rules return zero or more advice events.
/// Zero means the rule did not fire for this event.
pub type RuleOutput = Vec<AdviceEvent>;

// ---------------------------------------------------------------------------
// Convenience constructor so rules don't repeat boilerplate
// ---------------------------------------------------------------------------

pub fn advice(
    key:      &str,
    title:    &str,
    message:  String,
    severity: Severity,
    kv:       Vec<(String, String)>,
    now_ms:   u64,
) -> AdviceEvent {
    AdviceEvent {
        key:          key.to_owned(),
        title:        title.to_owned(),
        message,
        severity,
        kv,
        timestamp_ms: now_ms,
    }
}
