# CombatLedger — Live Coaching (12.0+ compatible)  
*A product roadmap + core capabilities + technology stack for a Companion-App-first live coaching system.*  

---

## 1) Product goal (what “live coaching” means)
Deliver **real-time, low-noise, high-confidence guidance** that helps a player improve **during pulls** and **immediately after wipes**, without relying on restricted/secret in-game APIs.

**North-star experience**
- During pull: 1–3 “Now” items, prioritized, throttled
- After wipe/kill: 10-second debrief, then optional deeper review
- Over time: personalized patterns (your baseline vs this pull)

---

## 2) Core capabilities (must-have)
### A) Data ingestion (Companion App)
1. **CombatLog tailing**
   - Tail `WoWCombatLog.txt` (or the active logging file)
   - Detect log rotation / restarts / toggles
2. **Fast parse + normalize**
   - Parse each line → typed event struct
   - Maintain stable schemas across versions (unit tests)
3. **Session & pull segmentation**
   - Detect “pull start/end” heuristics (combat start, boss engage, wipe)
   - Store pull boundaries and key summaries

### B) Identity & targeting (make “right person” easy)
4. **Identity handshake (tiny addon recommended)**
   - Send `playerGUID`, name, realm, class/spec, instance hints
   - Companion maps “who to coach” with zero ambiguity
5. **Event routing**
   - Self stream (source/dest GUID matches player)
   - Encounter stream (boss casts, phase signals)
   - Role stream (interrupt assignments, externals, dispels)

### C) Coaching engine (the “brain”)
6. **Stateful combat model**
   - Rolling windows (last 10–30s)
   - Cast history, cooldown inference (from observed usage)
   - Avoidable damage counters (by spellId)
   - GCD gap / uptime estimates
7. **Advice rules with safeguards**
   - Confidence scoring (high/medium/low)
   - Dedup + per-advice cooldowns
   - “Never claim availability” unless derived from observed casts
8. **Output policy**
   - Intensity slider (quiet → aggressive)
   - Per-rule enable/disable & priority

### D) UX (what the player sees)
9. **Overlay / second-screen UI**
   - “Now” feed (1–3 cards)
   - Timeline (last 30s)
   - Pull clock + phase label
10. **Post-pull 10-second debrief**
   - Top 1–3 issues
   - Next pull focus bullets
   - Optional deeper breakdown button

---

## 3) “Nice-to-have” capabilities (v2+)
- Encounter library (per boss rulesets, avoidable spell lists)
- Team view for raid lead (interrupt coverage, defensive coverage)
- Personal baseline tracking (“you vs you”)
- Web share links (pull debrief → URL)
- Voice coaching / stream deck integration
- “Coach pack” marketplace (community-authored rules)

---

## 4) What’s realistic under 12.0+ restrictions
**Design principle:** Treat the Companion App as the computation layer. Keep the addon thin:
- Identity/handshake
- Optional lightweight UI surfaces (non-computational)

Avoid in-addon features that rely on:
- Combat log processing in Lua (fragile in 12.0+ environments)
- Numeric comparisons on values that may become “secret”
- Anything that triggers forbidden/taint issues in combat

---

## 5) Recommended architecture
### A) Process layout
- **Companion Core (parser + state + coach engine)**
- **UI Shell (overlay renderer)**
- **Addon Bridge (optional, recommended)**

### B) Data flow
1. WoW writes CombatLog line
2. Companion tails → parses → updates state
3. Coach engine emits advice events (priority + throttle keys)
4. UI displays immediately (overlay / second screen)
5. On wipe/kill, summary generator runs in <10s

---

## 6) Technology stack (pragmatic choices)
### Option 1: Electron + Rust core (strong default)
- UI: Electron (React/TS) or Tauri (if you want lighter)
- Core: Rust (fast parsing, safe concurrency)
- Storage: SQLite (sessions, pulls, per-spec baselines)
- IPC: WebSocket or local named pipe between core ↔ UI
- Overlay: transparent always-on-top window; optional click-through mode

### Option 2: Desktop-only minimal footprint (Tauri + Rust)
- Better performance/memory profile
- Slightly more integration work for overlay behaviors

### Addon bridge
- Lua addon sends: GUID/name/spec/instance/encounter hints
- Transport: local loopback socket (if allowed) OR file drop + watcher OR addon-to-Companion via custom protocol
  - (Exact mechanism depends on what’s permitted and your preferred OS support.)

### Testing & reliability
- Golden-file tests: recorded combat logs → expected advice
- Fuzz parser: ensure no log line crashes the core
- Profiling: maintain low latency (<300ms typical) and low CPU

---

## 7) Roadmap (build order)
### Phase 0 — 1–2 weeks: “prove the loop”
- Tail file reliably (rotation, restarts)
- Parse core event types you need for MVP (damage taken, casts, interrupts)
- Basic UI: Now feed + pull clock
- 3 coaching rules:
  - Avoidable damage repeats
  - GCD gap detection
  - Cooldown drift (from observed usage)

### Phase 1 — 3–6 weeks: MVP Beta
- Session/pull segmentation
- Confidence scoring + throttling system
- Spec profile v1 (1–2 specs)
- Post-pull 10-second debrief
- Minimal addon handshake

### Phase 2 — 6–10 weeks: “Real raids”
- Encounter detection heuristics and phase markers
- Rule library UI (enable/disable, intensity, priorities)
- Baseline tracking (you vs you)
- Export/share (pull summaries)

### Phase 3 — 10–16 weeks: Differentiation
- Raid lead view (optional)
- Voice coaching / audio callouts
- Community coach packs / templating
- Cloud sync (optional; privacy-first)

---

## 8) Core MVP coaching set (recommended)
Start with advice that is **high confidence** from the log:
1. Avoidable hits (repeat counter)
2. Cooldown drift (timestamps only)
3. GCD gaps (uptime)
4. Missed interrupts (with confidence flag)
5. Defensive timing reinforcement (positive feedback)

---

## 9) Non-goals (keep MVP tight)
- Don’t try to recreate Warcraft Logs depth in v1
- Don’t attempt speculative “you had X available” claims
- Don’t spam; fewer, better messages win trust

---

## 10) Deliverables you can ship
- Desktop app + overlay + demo simulator
- Optional addon bridge
- A “Coach Library” with 10–20 curated rules per supported spec
