/// Coaching rule evaluator — the "brain" of the pipeline.
///
/// Receives typed LogEvents and PlayerIdentity updates via channels,
/// maintains CombatState, evaluates rules, deduplicates advice, and
/// forwards AdviceEvents to the IPC layer.
///
/// Per-rule advice cooldowns prevent spam:
///   bad    → 8s minimum between firings of the same key
///   warn   → 12s
///   good   → 20s
use crate::{
    config::AppConfig,
    identity::PlayerIdentity,
    ipc::StateSnapshot,
    parser::LogEvent,
    rules::{avoidable_repeat, cooldown_drift, gcd_gap, RuleContext, RuleInput},
    state::{CombatState, PullOutcome},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc::{Receiver, Sender};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Good,
    Warn,
    Bad,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdviceEvent {
    pub key:          String,
    pub title:        String,
    pub message:      String,
    pub severity:     Severity,
    pub kv:           Vec<(String, String)>,
    pub timestamp_ms: u64,
}

// ---------------------------------------------------------------------------
// Advice dedup / cooldown
// ---------------------------------------------------------------------------

fn advice_cooldown_ms(severity: &Severity) -> u64 {
    match severity {
        Severity::Bad  =>  8_000,
        Severity::Warn => 12_000,
        Severity::Good => 20_000,
    }
}

struct EngineState {
    combat:           CombatState,
    identity:         PlayerIdentity,
    config:           AppConfig,
    advice_last_ms:   HashMap<String, u64>,
}

impl EngineState {
    fn new(config: AppConfig) -> Self {
        Self {
            combat:         CombatState::new(),
            identity:       PlayerIdentity::unknown(),
            config,
            advice_last_ms: HashMap::new(),
        }
    }

    fn can_fire(&self, key: &str, severity: &Severity, now_ms: u64) -> bool {
        let cooldown = advice_cooldown_ms(severity);
        let last     = self.advice_last_ms.get(key).copied().unwrap_or(0);
        now_ms.saturating_sub(last) >= cooldown
    }

    fn mark_fired(&mut self, key: &str, now_ms: u64) {
        self.advice_last_ms.insert(key.to_owned(), now_ms);
    }
}

// ---------------------------------------------------------------------------
// Main engine task
// ---------------------------------------------------------------------------

pub async fn run(
    mut event_rx:  Receiver<LogEvent>,
    mut id_rx:     Receiver<PlayerIdentity>,
    advice_tx:     Sender<AdviceEvent>,
    snap_tx:       Sender<StateSnapshot>,
    config:        AppConfig,
) -> Result<()> {
    let mut eng = EngineState::new(config);

    loop {
        tokio::select! {
            // Identity updates are rare — process immediately
            Some(identity) = id_rx.recv() => {
                tracing::info!("Identity updated → {}/{}", identity.name, identity.spec);
                eng.combat.player_guid = Some(identity.guid.clone());
                eng.identity = identity;
            }

            // Combat log events — the hot path
            Some(event) = event_rx.recv() => {
                let now_ms = event.timestamp_ms();

                // Update the combat state machine for every event
                update_state(&mut eng.combat, &event, now_ms);

                // Only run coaching rules for events involving the coached player
                let coached = is_coached_event(&event, &eng.combat.player_guid);
                if coached {
                    let ctx = RuleContext {
                        state:     &eng.combat,
                        identity:  &eng.identity,
                        intensity: eng.config.intensity,
                        now_ms,
                    };
                    let input = RuleInput { event: &event };

                    let candidates: Vec<AdviceEvent> = avoidable_repeat::evaluate(&input, &ctx)
                        .into_iter()
                        .chain(gcd_gap::evaluate(&input, &ctx))
                        .chain(cooldown_drift::evaluate(&input, &ctx, &eng.config.major_cds))
                        .collect();

                    for advice in candidates {
                        if eng.can_fire(&advice.key, &advice.severity, now_ms) {
                            eng.mark_fired(&advice.key, now_ms);
                            if advice_tx.send(advice).await.is_err() {
                                return Ok(());
                            }
                        }
                    }
                }

                // Emit a state snapshot after every event for the UI widgets
                let snap = StateSnapshot {
                    pull_elapsed_ms: eng.combat.pull_elapsed_ms(now_ms),
                    gcd_gap_ms:      eng.combat.gcd.current_gap_ms,
                    avoidable_count: eng.combat.avoidable.total_hits(),
                    in_combat:       eng.combat.in_combat,
                };
                let _ = snap_tx.try_send(snap); // Non-blocking — drop if UI is slow
            }

            else => break,
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

fn is_coached_event(event: &LogEvent, player_guid: &Option<String>) -> bool {
    let guid = player_guid.as_deref();
    match event {
        LogEvent::SpellCastSuccess { source_guid, .. } => Some(source_guid.as_str()) == guid,
        LogEvent::SpellDamage { dest_guid, .. }        => Some(dest_guid.as_str()) == guid,
        LogEvent::SpellHeal { source_guid, .. }        => Some(source_guid.as_str()) == guid,
        LogEvent::SwingDamage { dest_guid, .. }        => Some(dest_guid.as_str()) == guid,
        LogEvent::SpellInterrupted { source_guid, .. } => Some(source_guid.as_str()) == guid,
        LogEvent::UnitDied { .. }                      => true, // Always process deaths
    }
}

fn update_state(state: &mut CombatState, event: &LogEvent, now_ms: u64) {
    match event {
        LogEvent::SpellCastSuccess { source_guid, spell_id, .. } => {
            // Combat start heuristic: first cast from any party member
            if !state.in_combat {
                state.start_pull(now_ms);
            }
            // Track GCD and cooldowns for the coached player
            if Some(source_guid.as_str()) == state.player_guid.as_deref() {
                state.gcd.record_cast(now_ms);
                state.cooldowns.record_cast(*spell_id, now_ms);
            }
        }

        LogEvent::SpellDamage { dest_guid, spell_id, .. } => {
            if Some(dest_guid.as_str()) == state.player_guid.as_deref() {
                state.avoidable.record_hit(*spell_id, now_ms);
            }
            state.event_window.push(event.clone(), now_ms);
        }

        LogEvent::UnitDied { dest_name, dest_guid, .. } => {
            if state.in_combat {
                // Phase 0 heuristic: treat any UNIT_DIED as pull end.
                // Phase 1: distinguish boss kill vs player wipe via GUID flags.
                let outcome = if dest_guid.starts_with("Creature") {
                    PullOutcome::Kill
                } else {
                    PullOutcome::Wipe
                };
                state.end_pull(now_ms, outcome);
                tracing::debug!("Pull ended by death of '{}'", dest_name);
            }
        }

        _ => {
            state.event_window.push(event.clone(), now_ms);
        }
    }
}
