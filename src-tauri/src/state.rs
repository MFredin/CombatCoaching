/// Stateful combat model — maintained by the engine, read by rule evaluators.
///
/// All state lives in a single CombatState owned by the engine task.
/// No locking is needed because the engine is single-threaded.
use crate::parser::LogEvent;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Pull tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    Kill,
    Wipe,
}

#[derive(Debug, Clone)]
pub struct Pull {
    pub pull_number: u32,
    pub start_ms:    u64,
    pub end_ms:      Option<u64>,
    pub outcome:     Option<PullOutcome>,
}

// ---------------------------------------------------------------------------
// Rolling event window (last N milliseconds)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WindowedEvent {
    pub timestamp_ms: u64,
    pub event:        LogEvent,
}

#[derive(Debug)]
pub struct EventWindow {
    pub events:    Vec<WindowedEvent>,
    pub window_ms: u64,
}

impl EventWindow {
    pub fn new(window_ms: u64) -> Self {
        Self { events: Vec::new(), window_ms }
    }

    pub fn push(&mut self, event: LogEvent, now_ms: u64) {
        self.events.push(WindowedEvent { timestamp_ms: now_ms, event });
        let cutoff = now_ms.saturating_sub(self.window_ms);
        self.events.retain(|e| e.timestamp_ms >= cutoff);
    }
}

// ---------------------------------------------------------------------------
// Avoidable damage tracker
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct AvoidableTracker {
    /// spell_id -> hit count this pull
    pub hit_counts:     HashMap<u32, u32>,
    /// spell_id -> timestamps of each hit
    pub hit_timestamps: HashMap<u32, Vec<u64>>,
}

impl AvoidableTracker {
    pub fn record_hit(&mut self, spell_id: u32, timestamp_ms: u64) {
        *self.hit_counts.entry(spell_id).or_insert(0) += 1;
        self.hit_timestamps.entry(spell_id).or_default().push(timestamp_ms);
    }

    pub fn hit_count(&self, spell_id: u32) -> u32 {
        self.hit_counts.get(&spell_id).copied().unwrap_or(0)
    }

    pub fn total_hits(&self) -> u32 {
        self.hit_counts.values().sum()
    }

    pub fn reset(&mut self) {
        self.hit_counts.clear();
        self.hit_timestamps.clear();
    }
}

// ---------------------------------------------------------------------------
// Cooldown tracker (inferred from observed SPELL_CAST_SUCCESS)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct CooldownTracker {
    /// spell_id -> last observed use timestamp
    pub last_used: HashMap<u32, u64>,
}

impl CooldownTracker {
    pub fn record_cast(&mut self, spell_id: u32, timestamp_ms: u64) {
        self.last_used.insert(spell_id, timestamp_ms);
    }

    /// How long ago was this spell last cast? None = never seen this pull.
    pub fn elapsed_since_last(&self, spell_id: u32, now_ms: u64) -> Option<u64> {
        self.last_used.get(&spell_id).map(|&t| now_ms.saturating_sub(t))
    }

    pub fn last_used_ms(&self, spell_id: u32) -> Option<u64> {
        self.last_used.get(&spell_id).copied()
    }

    pub fn reset(&mut self) {
        self.last_used.clear();
    }
}

// ---------------------------------------------------------------------------
// GCD gap tracker
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct GcdTracker {
    pub last_cast_ms:    Option<u64>,
    /// Gap in ms between the last two casts
    pub current_gap_ms:  u64,
}

impl GcdTracker {
    pub fn record_cast(&mut self, timestamp_ms: u64) {
        if let Some(last) = self.last_cast_ms {
            self.current_gap_ms = timestamp_ms.saturating_sub(last);
        }
        self.last_cast_ms = Some(timestamp_ms);
    }

    pub fn reset(&mut self) {
        self.last_cast_ms   = None;
        self.current_gap_ms = 0;
    }
}

// ---------------------------------------------------------------------------
// Top-level CombatState
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct CombatState {
    pub current_pull:  Option<Pull>,
    pub pull_history:  Vec<Pull>,
    pub event_window:  EventWindow,
    pub avoidable:     AvoidableTracker,
    pub cooldowns:     CooldownTracker,
    pub gcd:           GcdTracker,
    pub in_combat:     bool,
    pub player_guid:   Option<String>,
}

impl CombatState {
    pub fn new() -> Self {
        Self {
            current_pull:  None,
            pull_history:  Vec::new(),
            event_window:  EventWindow::new(30_000),
            avoidable:     AvoidableTracker::default(),
            cooldowns:     CooldownTracker::default(),
            gcd:           GcdTracker::default(),
            in_combat:     false,
            player_guid:   None,
        }
    }

    pub fn start_pull(&mut self, timestamp_ms: u64) {
        let n = (self.pull_history.len() as u32) + 1;
        self.current_pull = Some(Pull {
            pull_number: n,
            start_ms:    timestamp_ms,
            end_ms:      None,
            outcome:     None,
        });
        self.avoidable.reset();
        self.cooldowns.reset();
        self.gcd.reset();
        self.in_combat = true;
        tracing::info!("Pull {} started at {}ms", n, timestamp_ms);
    }

    pub fn end_pull(&mut self, timestamp_ms: u64, outcome: PullOutcome) {
        if let Some(mut pull) = self.current_pull.take() {
            pull.end_ms  = Some(timestamp_ms);
            pull.outcome = Some(outcome.clone());
            self.pull_history.push(pull);
        }
        self.in_combat = false;
        tracing::info!("Pull ended: {:?}", outcome);
    }

    /// Milliseconds elapsed since pull start. Returns 0 if not in a pull.
    pub fn pull_elapsed_ms(&self, now_ms: u64) -> u64 {
        self.current_pull
            .as_ref()
            .map(|p| now_ms.saturating_sub(p.start_ms))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_lifecycle() {
        let mut state = CombatState::new();
        assert!(!state.in_combat);

        state.start_pull(1000);
        assert!(state.in_combat);
        assert_eq!(state.pull_elapsed_ms(3000), 2000);

        state.end_pull(5000, PullOutcome::Wipe);
        assert!(!state.in_combat);
        assert_eq!(state.pull_history.len(), 1);
        assert_eq!(state.pull_history[0].outcome, Some(PullOutcome::Wipe));
    }

    #[test]
    fn avoidable_tracker() {
        let mut tracker = AvoidableTracker::default();
        tracker.record_hit(12345, 1000);
        tracker.record_hit(12345, 2000);
        assert_eq!(tracker.hit_count(12345), 2);
        tracker.reset();
        assert_eq!(tracker.hit_count(12345), 0);
    }

    #[test]
    fn gcd_gap() {
        let mut gcd = GcdTracker::default();
        gcd.record_cast(1000);
        gcd.record_cast(3500);
        assert_eq!(gcd.current_gap_ms, 2500);
    }
}
