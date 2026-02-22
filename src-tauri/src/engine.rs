/// Coaching rule evaluator — the "brain" of the pipeline.
///
/// Receives typed LogEvents and PlayerIdentity updates via channels,
/// maintains CombatState, evaluates rules, deduplicates advice, and
/// forwards AdviceEvents to the IPC layer and SQLite DB.
///
/// Per-rule advice cooldowns prevent spam:
///   bad    → 8s minimum between firings of the same key
///   warn   → 12s
///   good   → 20s
///
/// GUID inference: if the addon is not installed, the engine infers the
/// player GUID from the first SPELL_CAST_SUCCESS whose source_name matches
/// the `player_focus` character name stored in AppConfig.
use crate::{
    config::AppConfig,
    db::DbWriter,
    identity::PlayerIdentity,
    ipc::StateSnapshot,
    parser::LogEvent,
    rules::{avoidable_repeat, cooldown_drift, gcd_gap, RuleContext, RuleInput},
    specs,
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
    combat:              CombatState,
    identity:            PlayerIdentity,
    config:              AppConfig,
    advice_last_ms:      HashMap<String, u64>,
    db:                  DbWriter,
    session_id:          i64,
    current_pull_id:     Option<i64>,
    pull_number:         u32,
    /// Resolved major CD IDs — from spec profile (auto-detected or user-selected).
    /// Falls back to `config.major_cds` if no spec profile is loaded.
    effective_major_cds: Vec<u32>,
    /// Resolved active mitigation IDs — from spec profile (for future rules).
    #[allow(dead_code)]
    effective_am_spells: Vec<u32>,
    /// Character name extracted from `config.player_focus` for GUID inference.
    focus_name:          String,
    /// Total advice events fired this pull (for future debrief).
    #[allow(dead_code)]
    pull_advice_count:   u32,
}

impl EngineState {
    fn new(config: AppConfig, db: DbWriter, session_id: i64) -> Self {
        // If a spec was pre-selected in config, resolve CDs immediately.
        let (effective_major_cds, effective_am_spells) = if !config.selected_spec.is_empty() {
            if let Some(profile) = specs::load_by_key(&config.selected_spec) {
                (profile.major_cd_spell_ids, profile.am_spell_ids)
            } else {
                (config.major_cds.clone(), Vec::new())
            }
        } else if !config.major_cds.is_empty() {
            (config.major_cds.clone(), Vec::new())
        } else {
            (Vec::new(), Vec::new())
        };

        // Extract just the character name from "Name-Realm" format.
        let focus_name = config
            .player_focus
            .split('-')
            .next()
            .unwrap_or("")
            .to_owned();

        Self {
            combat:              CombatState::new(),
            identity:            PlayerIdentity::unknown(),
            advice_last_ms:      HashMap::new(),
            db,
            session_id,
            current_pull_id:     None,
            pull_number:         0,
            effective_major_cds,
            effective_am_spells,
            focus_name,
            pull_advice_count:   0,
            config,
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
    db:            DbWriter,
) -> Result<()> {
    // Insert a session row before entering the hot loop.
    let session_start_ms = unix_now_ms();
    let session_id = db
        .insert_session(session_start_ms, String::new(), String::new())
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("DB insert_session failed: {}", e);
            -1
        });
    tracing::info!("DB session {} started", session_id);

    let mut eng = EngineState::new(config, db, session_id);

    loop {
        tokio::select! {
            // Identity updates are rare — process immediately
            Some(identity) = id_rx.recv() => {
                tracing::info!("Identity updated → {}/{}", identity.name, identity.spec);
                eng.combat.player_guid = Some(identity.guid.clone());

                // Auto-load spec profile if user has not explicitly selected one.
                if eng.config.selected_spec.is_empty() {
                    if let Some(profile) = specs::load_spec(&identity.class, &identity.spec) {
                        tracing::info!(
                            "Auto-loaded spec {}: {} major CD IDs, {} AM IDs",
                            profile.key(),
                            profile.major_cd_spell_ids.len(),
                            profile.am_spell_ids.len()
                        );
                        eng.effective_major_cds = profile.major_cd_spell_ids;
                        eng.effective_am_spells = profile.am_spell_ids;
                    } else {
                        tracing::debug!(
                            "No spec profile for {}/{} — cooldown_drift will not fire",
                            identity.class, identity.spec
                        );
                    }
                }

                eng.identity = identity;
            }

            // Combat log events — the hot path
            Some(event) = event_rx.recv() => {
                let now_ms = event.timestamp_ms();

                // GUID inference: if no identity yet but player_focus is configured,
                // try to infer GUID from the first matching SPELL_CAST_SUCCESS.
                if eng.combat.player_guid.is_none() && !eng.focus_name.is_empty() {
                    if let LogEvent::SpellCastSuccess { source_guid, source_name, .. } = &event {
                        if source_name.eq_ignore_ascii_case(&eng.focus_name) {
                            tracing::info!(
                                "GUID inferred from player_focus '{}': {}",
                                eng.focus_name, source_guid
                            );
                            eng.combat.player_guid = Some(source_guid.clone());
                        }
                    }
                }

                // Snapshot in_combat before state mutation to detect transitions
                let was_in_combat = eng.combat.in_combat;

                // Update the combat state machine for every event
                update_state(&mut eng.combat, &event, now_ms);

                // ── Pull start ─────────────────────────────────────────────────
                if !was_in_combat && eng.combat.in_combat {
                    eng.pull_number      += 1;
                    eng.pull_advice_count = 0;
                    let pn  = eng.pull_number;
                    let sid = eng.session_id;
                    match eng.db.insert_pull(sid, pn, now_ms).await {
                        Ok(id) => {
                            tracing::info!("DB pull {} started (id={})", pn, id);
                            eng.current_pull_id = Some(id);
                        }
                        Err(e) => tracing::warn!("DB insert_pull failed: {}", e),
                    }
                }

                // ── Pull end ───────────────────────────────────────────────────
                if was_in_combat && !eng.combat.in_combat {
                    if let Some(pull_id) = eng.current_pull_id.take() {
                        let outcome = eng.combat.pull_history.last()
                            .and_then(|p| p.outcome.as_ref())
                            .map(|o| format!("{:?}", o).to_lowercase())
                            .unwrap_or_else(|| "unknown".to_string());
                        eng.db.end_pull(pull_id, now_ms, outcome);
                    }
                    // Reset per-pull dedup so rules fire fresh next pull
                    eng.advice_last_ms.clear();
                }

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
                        .chain(cooldown_drift::evaluate(&input, &ctx, &eng.effective_major_cds))
                        .collect();

                    for advice in candidates {
                        if eng.can_fire(&advice.key, &advice.severity, now_ms) {
                            eng.mark_fired(&advice.key, now_ms);
                            eng.pull_advice_count += 1;

                            // Persist to DB (fire-and-forget)
                            if let Some(pull_id) = eng.current_pull_id {
                                eng.db.insert_advice(
                                    pull_id,
                                    now_ms,
                                    advice.key.clone(),
                                    format!("{:?}", advice.severity).to_lowercase(),
                                    advice.message.clone(),
                                );
                            }

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
                    interrupt_count: eng.combat.interrupt_count,
                    encounter_name:  eng.combat.encounter_name.clone(),
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
        LogEvent::UnitDied { .. }                      => true,
        LogEvent::EncounterStart { .. }                => true,
        LogEvent::EncounterEnd { .. }                  => true,
        LogEvent::SpellCastFailed { source_guid, .. } => Some(source_guid.as_str()) == guid,
        LogEvent::SpellCastStart { source_guid, .. }  => Some(source_guid.as_str()) == guid,
    }
}

fn update_state(state: &mut CombatState, event: &LogEvent, now_ms: u64) {
    match event {
        LogEvent::SpellCastSuccess { source_guid, spell_id, .. } => {
            // Combat start heuristic: first cast from any party member.
            // EncounterStart is the preferred signal; this is the fallback.
            if !state.in_combat {
                state.start_pull(now_ms);
            }
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
            // Fall back to UNIT_DIED as pull-end signal only when not in an
            // encounter (ENCOUNTER_END is authoritative and handled below).
            if state.in_combat && state.encounter_name.is_none() {
                let outcome = if dest_guid.starts_with("Creature") {
                    PullOutcome::Kill
                } else {
                    PullOutcome::Wipe
                };
                state.end_pull(now_ms, outcome);
                tracing::debug!("Pull ended by UNIT_DIED '{}'", dest_name);
            }
        }

        LogEvent::SpellInterrupted { source_guid, .. } => {
            if Some(source_guid.as_str()) == state.player_guid.as_deref() {
                state.interrupt_count += 1;
            }
            state.event_window.push(event.clone(), now_ms);
        }

        LogEvent::EncounterStart { encounter_name, .. } => {
            tracing::info!("ENCOUNTER_START: {}", encounter_name);
            state.encounter_name = Some(encounter_name.clone());
            if !state.in_combat {
                state.start_pull(now_ms);
            }
        }

        LogEvent::EncounterEnd { encounter_name, success, .. } => {
            tracing::info!("ENCOUNTER_END: {} success={}", encounter_name, success);
            if state.in_combat {
                let outcome = if *success { PullOutcome::Kill } else { PullOutcome::Wipe };
                state.end_pull(now_ms, outcome);
            }
            state.encounter_name = None;
        }

        LogEvent::SpellCastFailed { .. } | LogEvent::SpellCastStart { .. } => {
            state.event_window.push(event.clone(), now_ms);
        }

        _ => {
            state.event_window.push(event.clone(), now_ms);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
