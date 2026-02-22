// Entry point for the settings window.
// Tabbed layout: Home | Live Feed | Audio | Hotkeys
import React, { useState, useEffect, useCallback, useRef } from "react";
import { createRoot } from "react-dom/client";
import { invoke }     from "@tauri-apps/api/core";
import { open }       from "@tauri-apps/plugin-dialog";
import { ConnectionStatus }    from "./components/ConnectionStatus";
import { OverlayLayoutEditor } from "./components/OverlayLayoutEditor";
import { useTauriEvents }      from "./hooks/useTauriEvents";
import type {
  AppConfig,
  AudioCue,
  AdviceEvent,
  ConnectionStatus as ConnStatus,
  PanelPosition,
  StateSnapshot,
  UpdateInfo,
  WtfCharacter,
} from "./types/events";
import "./styles/settings.css";

// ---------------------------------------------------------------------------
// Tab definitions
// ---------------------------------------------------------------------------
type Tab = "home" | "livefeed" | "audio" | "hotkeys";

// ---------------------------------------------------------------------------
// Root settings app
// ---------------------------------------------------------------------------
function SettingsApp() {
  const [tab, setTab]                 = useState<Tab>("home");
  const [config, setConfig]           = useState<AppConfig | null>(null);
  const [connStatus, setConnStatus]   = useState<ConnStatus>({
    log_tailing: false, addon_connected: false, wow_path: "",
  });
  const [saving, setSaving]           = useState(false);
  const [detectMsg, setDetectMsg]     = useState("");
  const [wtfChars, setWtfChars]       = useState<WtfCharacter[]>([]);
  const [updateInfo, setUpdateInfo]   = useState<UpdateInfo | null>(null);
  const [updateChecking, setChecking] = useState(false);
  // Live Feed state
  const [advice, setAdvice]           = useState<AdviceEvent[]>([]);
  const [snapshot, setSnapshot]       = useState<StateSnapshot>({
    pull_elapsed_ms: 0, gcd_gap_ms: 0, avoidable_count: 0, in_combat: false, interrupt_count: 0,
  });
  const [eventCount, setEventCount]   = useState(0);
  const [overlayOn, setOverlayOn]     = useState(true);

  // Load config on mount
  useEffect(() => {
    invoke<AppConfig>("get_config").then((cfg) => {
      setConfig(cfg);
      setOverlayOn(cfg.overlay_visible ?? true);
    }).catch(console.error);
  }, []);

  // Reload WTF character list when log path changes
  useEffect(() => {
    if (config?.wow_log_path) {
      invoke<WtfCharacter[]>("list_wtf_characters")
        .then(setWtfChars).catch(() => setWtfChars([]));
    } else {
      setWtfChars([]);
    }
  }, [config?.wow_log_path]);

  useTauriEvents({
    onConnection:    useCallback((s: ConnStatus)    => setConnStatus(s), []),
    onStateSnapshot: useCallback((s: StateSnapshot) => setSnapshot(s),   []),
    onAdvice: useCallback((a: AdviceEvent) => {
      setEventCount((n) => n + 1);
      setAdvice((prev) => [a, ...prev.filter((x) => x.key !== a.key)].slice(0, 50));
    }, []),
  });

  if (!config) {
    return <div style={{ padding: 32, color: "var(--muted)" }}>Loading…</div>;
  }

  // -------------------------------------------------------------------------
  // Config helpers
  // -------------------------------------------------------------------------
  async function save(updated: AppConfig) {
    setSaving(true);
    try {
      await invoke("save_config", { config: updated });
      setConfig(updated);
    } finally {
      setSaving(false);
    }
  }

  async function detectPath() {
    const found = await invoke<string | null>("detect_wow_path");
    if (found) {
      await save({ ...config, wow_log_path: found });
      setDetectMsg(`Found: ${found}`);
    } else {
      setDetectMsg("Not found automatically. Browse to your WoW Logs folder.");
    }
  }

  async function browsePath() {
    const selected = await open({ directory: true, title: "Select WoW Logs Folder" });
    if (typeof selected === "string") await save({ ...config, wow_log_path: selected });
  }

  async function browseAddonPath() {
    const selected = await open({
      filters: [{ name: "Lua Saved Variables", extensions: ["lua"] }],
      title: "Select CombatCoach.lua SavedVariables",
    });
    if (typeof selected === "string") await save({ ...config, addon_sv_path: selected });
  }

  async function checkForUpdates() {
    setChecking(true); setUpdateInfo(null);
    try {
      setUpdateInfo(await invoke<UpdateInfo>("check_for_update"));
    } catch (e) {
      setUpdateInfo({ available: false, current_version: "?", new_version: null, notes: String(e) });
    } finally {
      setChecking(false);
    }
  }

  useEffect(() => {
    const t = setTimeout(() => { void checkForUpdates(); }, 5_000);
    return () => clearTimeout(t);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function updatePanels(positions: PanelPosition[]) {
    const updated = { ...config, panel_positions: positions };
    setConfig(updated);
    void save(updated);
  }

  async function toggleOverlay() {
    const newVisible = await invoke<boolean>("toggle_overlay");
    setOverlayOn(newVisible);
    setConfig((c) => c ? { ...c, overlay_visible: newVisible } : c);
  }

  // -------------------------------------------------------------------------
  // Render
  // -------------------------------------------------------------------------
  const intensityLabels: Record<number, string> = {
    1: "Quiet — critical mistakes only",
    2: "Low — major mistakes + big wins",
    3: "Balanced — clear-value callouts",
    4: "High — uptime gaps + interrupts",
    5: "Maximum — aggressive, frequent",
  };

  const TAB_LABELS: Record<Tab, string> = {
    home:     "⚙ Home",
    livefeed: "📡 Live Feed",
    audio:    "🔊 Audio",
    hotkeys:  "⌨ Hotkeys",
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
      {/* ── Top bar ── */}
      <header style={{
        display: "flex", alignItems: "center", gap: 0,
        borderBottom: "1px solid var(--stroke)",
        background: "rgba(11,15,24,0.97)",
        flexShrink: 0,
      }}>
        {/* Logo */}
        <div style={{ padding: "0 20px", borderRight: "1px solid var(--stroke)", minWidth: 180 }}>
          <div style={{ fontWeight: 700, fontSize: 14, lineHeight: "42px" }}>CombatLedger</div>
          <div style={{ fontSize: 10, color: "var(--muted)", marginTop: -8, paddingBottom: 6 }}>
            Live Coach v0.8.5
          </div>
        </div>

        {/* Tabs */}
        {(Object.keys(TAB_LABELS) as Tab[]).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            style={{
              borderRadius: 0, border: "none",
              borderBottom: tab === t ? "2px solid var(--accent)" : "2px solid transparent",
              background: "none",
              color: tab === t ? "var(--text)" : "var(--muted)",
              fontWeight: tab === t ? 600 : 400,
              fontSize: 13, padding: "0 18px", height: 48, cursor: "pointer",
            }}
          >
            {TAB_LABELS[t]}
          </button>
        ))}

        {/* Right: status + overlay toggle */}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 10, paddingRight: 16 }}>
          <ConnectionStatus status={connStatus} />
          <button
            onClick={toggleOverlay}
            className={overlayOn ? "primary" : ""}
            style={{ fontSize: 11, padding: "6px 12px" }}
            title="Toggle overlay visibility"
          >
            {overlayOn ? "Overlay ON" : "Overlay OFF"}
          </button>
          {saving && <span style={{ fontSize: 11, color: "var(--muted)" }}>Saving…</span>}
        </div>
      </header>

      {/* ── Tab content ── */}
      <div style={{ flex: 1, overflow: "auto" }}>
        {tab === "home"     && (
          <HomeTab
            config={config} setConfig={setConfig} save={save}
            wtfChars={wtfChars} detectPath={detectPath}
            browsePath={browsePath} browseAddonPath={browseAddonPath}
            detectMsg={detectMsg} updateInfo={updateInfo}
            updateChecking={updateChecking} checkForUpdates={checkForUpdates}
            intensityLabels={intensityLabels} updatePanels={updatePanels}
          />
        )}
        {tab === "livefeed" && (
          <LiveFeedTab
            advice={advice} snapshot={snapshot}
            eventCount={eventCount} connStatus={connStatus}
          />
        )}
        {tab === "audio"    && <AudioTab   config={config} save={save} />}
        {tab === "hotkeys"  && (
          <HotkeysTab
            config={config} save={save}
            overlayOn={overlayOn} toggleOverlay={toggleOverlay}
          />
        )}
      </div>
    </div>
  );
}

// ===========================================================================
// HOME TAB
// ===========================================================================
interface HomeTabProps {
  config:          AppConfig;
  setConfig:       (c: AppConfig) => void;
  save:            (c: AppConfig) => Promise<void>;
  wtfChars:        WtfCharacter[];
  detectPath:      () => void;
  browsePath:      () => void;
  browseAddonPath: () => void;
  detectMsg:       string;
  updateInfo:      UpdateInfo | null;
  updateChecking:  boolean;
  checkForUpdates: () => void;
  intensityLabels: Record<number, string>;
  updatePanels:    (p: PanelPosition[]) => void;
}

function HomeTab({
  config, setConfig, save, wtfChars,
  detectPath, browsePath, browseAddonPath, detectMsg,
  updateInfo, updateChecking, checkForUpdates,
  intensityLabels, updatePanels,
}: HomeTabProps) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "260px 1fr", height: "100%" }}>
      {/* Sidebar */}
      <aside style={{ borderRight: "1px solid var(--stroke)", overflow: "auto" }}>

        <div className="section">
          <h3>WoW Logs Folder</h3>
          <div style={{ fontSize: 11, color: "var(--muted)", wordBreak: "break-all", marginBottom: 4 }}>
            {config.wow_log_path || "Not configured"}
          </div>
          <div style={{ fontSize: 10, color: "var(--muted)", marginBottom: 8, fontStyle: "italic" }}>
            Newest WoWCombatLog*.txt is tailed automatically.
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button onClick={detectPath}>Auto-detect</button>
            <button onClick={browsePath}>Browse…</button>
          </div>
          {detectMsg && (
            <div style={{ fontSize: 11, color: "var(--muted)", marginTop: 6 }}>{detectMsg}</div>
          )}
        </div>

        <div className="section">
          <h3>Coached Character</h3>
          {wtfChars.length > 0 ? (
            <>
              <div style={{ fontSize: 10, color: "var(--muted)", marginBottom: 6, fontStyle: "italic" }}>
                Detected from WTF folder — no addon required.
              </div>
              <select
                value={config.player_focus ?? ""}
                onChange={(e) => {
                  const updated = { ...config, player_focus: e.target.value };
                  setConfig(updated);
                  void save(updated);
                }}
                style={{ width: "100%", fontSize: 12 }}
              >
                <option value="">— Auto-detect from log —</option>
                {wtfChars.map((c) => (
                  <option key={`${c.name}-${c.realm}`} value={`${c.name}-${c.realm}`}>
                    {c.name} ({c.realm})
                  </option>
                ))}
              </select>
            </>
          ) : (
            <>
              <div style={{ fontSize: 10, color: "var(--muted)", marginBottom: 6, fontStyle: "italic" }}>
                {config.wow_log_path ? "No characters found in WTF folder." : "Set Logs folder above first."}
              </div>
              <div style={{ fontSize: 11, color: "var(--muted)", wordBreak: "break-all", marginBottom: 6 }}>
                Addon SVars: {config.addon_sv_path || "Not configured"}
              </div>
              <button onClick={browseAddonPath}>Browse addon SVars…</button>
            </>
          )}
        </div>

        <div className="section">
          <h3>Coaching Intensity</h3>
          <input
            type="range" min={1} max={5} step={1}
            value={config.intensity ?? 3}
            onChange={(e) => {
              const updated = { ...config, intensity: Number(e.target.value) };
              setConfig(updated);
              void save(updated);
            }}
          />
          <div style={{ fontSize: 11, color: "var(--muted)", marginTop: 4 }}>
            {intensityLabels[config.intensity ?? 3] ?? ""}
          </div>
        </div>

        <div className="section">
          <h3>Updates</h3>
          {updateInfo?.available ? (
            <div style={{ fontSize: 11, marginBottom: 8 }}>
              <div style={{ color: "var(--good)", fontWeight: 600, marginBottom: 4 }}>
                ↑ Update available: v{updateInfo.new_version}
              </div>
              {updateInfo.notes && (
                <div style={{ color: "var(--muted)", whiteSpace: "pre-wrap", marginBottom: 6, maxHeight: 80, overflow: "auto" }}>
                  {updateInfo.notes}
                </div>
              )}
              <div style={{ fontSize: 10, color: "var(--muted)" }}>Restart the app after installing.</div>
            </div>
          ) : updateInfo && !updateInfo.available ? (
            <div style={{ fontSize: 11, color: "var(--muted)", marginBottom: 8 }}>
              ✓ Up to date (v{updateInfo.current_version})
            </div>
          ) : null}
          <button onClick={checkForUpdates} disabled={updateChecking}>
            {updateChecking ? "Checking…" : "Check for Updates"}
          </button>
        </div>
      </aside>

      {/* Main: overlay layout editor */}
      <main style={{ overflow: "auto", padding: 24 }}>
        <h2 style={{ margin: "0 0 6px 0", fontSize: 16 }}>Overlay Layout</h2>
        <p style={{ margin: "0 0 16px 0", fontSize: 12, color: "var(--muted)" }}>
          Drag panels to reposition. Use the sliders below to adjust opacity and scale per panel.
          Changes are saved immediately.
        </p>
        <OverlayLayoutEditor
          positions={config.panel_positions ?? []}
          onPositionChange={updatePanels}
        />
      </main>
    </div>
  );
}

// ===========================================================================
// LIVE FEED TAB
// ===========================================================================
interface LiveFeedTabProps {
  advice:     AdviceEvent[];
  snapshot:   StateSnapshot;
  eventCount: number;
  connStatus: ConnStatus;
}

function LiveFeedTab({ advice, snapshot, eventCount, connStatus }: LiveFeedTabProps) {
  const feedRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (feedRef.current) feedRef.current.scrollTop = 0;
  }, [advice.length]);

  const elapsed    = snapshot.pull_elapsed_ms;
  const mins       = Math.floor(elapsed / 60000);
  const secs       = Math.floor((elapsed % 60000) / 1000);
  const elapsedStr = elapsed > 0 ? `${mins}:${secs.toString().padStart(2, "0")}` : "—";

  const sevColor: Record<string, string> = {
    good: "var(--good)",
    warn: "var(--warn)",
    bad:  "var(--bad)",
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "200px 1fr", height: "100%" }}>
      {/* Left: stat sidebar */}
      <aside style={{
        borderRight: "1px solid var(--stroke)",
        padding: "16px 16px",
        display: "flex", flexDirection: "column", gap: 0,
        overflow: "auto",
      }}>
        <SectionHeader>Pipeline</SectionHeader>
        <StatBlock label="Log tailing"
          value={connStatus.log_tailing ? "Active" : "Inactive"}
          color={connStatus.log_tailing ? "var(--good)" : "var(--bad)"} />
        <StatBlock label="Addon"
          value={connStatus.addon_connected ? "Connected" : "Disconnected"}
          color={connStatus.addon_connected ? "var(--good)" : "var(--muted)"} />
        <StatBlock label="Events received"
          value={eventCount.toLocaleString()} />

        <SectionHeader style={{ marginTop: 16 }}>Combat State</SectionHeader>
        <StatBlock label="Status"
          value={snapshot.in_combat ? "In combat" : "Out of combat"}
          color={snapshot.in_combat ? "var(--warn)" : "var(--muted)"} />
        <StatBlock label="Pull time"    value={elapsedStr} />
        <StatBlock label="GCD gap"
          value={snapshot.gcd_gap_ms > 0 ? `${snapshot.gcd_gap_ms} ms` : "—"} />
        <StatBlock label="Avoidable hits"
          value={snapshot.avoidable_count.toString()} />
        <StatBlock label="Interrupts"
          value={snapshot.interrupt_count.toString()}
          color={snapshot.interrupt_count > 0 ? "var(--good)" : undefined} />

        <SectionHeader style={{ marginTop: 16 }}>Feed</SectionHeader>
        <StatBlock label="Cards shown"  value={advice.length.toString()} />
        <StatBlock label="Good"         value={advice.filter((a) => a.severity === "good").length.toString()} color="var(--good)" />
        <StatBlock label="Warnings"     value={advice.filter((a) => a.severity === "warn").length.toString()} color="var(--warn)" />
        <StatBlock label="Errors"       value={advice.filter((a) => a.severity === "bad").length.toString()}  color="var(--bad)" />
      </aside>

      {/* Right: live advice feed */}
      <main style={{ display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <div style={{
          padding: "10px 16px", borderBottom: "1px solid var(--stroke)",
          display: "flex", alignItems: "center", justifyContent: "space-between",
          flexShrink: 0,
        }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>Live Advice Feed</div>
          <div style={{ fontSize: 11, color: "var(--muted)" }}>
            {advice.length} card{advice.length !== 1 ? "s" : ""} · newest first
          </div>
        </div>

        <div
          ref={feedRef}
          style={{ flex: 1, overflow: "auto", padding: 16, display: "flex", flexDirection: "column", gap: 8 }}
        >
          {advice.length === 0 ? (
            <div style={{ color: "var(--muted)", fontSize: 12, fontStyle: "italic", paddingTop: 8 }}>
              No events yet — start a pull in WoW to see coaching events here.
            </div>
          ) : advice.map((a) => (
            <div key={a.key} style={{
              background: "rgba(255,255,255,0.03)",
              border: `1px solid ${sevColor[a.severity] ?? "var(--stroke)"}44`,
              borderLeft: `3px solid ${sevColor[a.severity] ?? "var(--stroke)"}`,
              borderRadius: 8, padding: "10px 14px",
            }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
                <span style={{
                  fontSize: 10, fontWeight: 700, letterSpacing: 1,
                  color: sevColor[a.severity], textTransform: "uppercase",
                }}>
                  {a.severity}
                </span>
                <span style={{ fontSize: 13, fontWeight: 600 }}>{a.title}</span>
                <span style={{ marginLeft: "auto", fontSize: 10, color: "var(--muted)" }}>
                  {new Date(a.timestamp_ms).toLocaleTimeString()}
                </span>
              </div>
              <div style={{ fontSize: 12, color: "var(--muted)", marginBottom: a.kv.length > 0 ? 6 : 0 }}>
                {a.message}
              </div>
              {a.kv.length > 0 && (
                <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 12px" }}>
                  {a.kv.map(([k, v]) => (
                    <span key={k} style={{ fontSize: 11, color: "var(--muted)" }}>
                      <span style={{ color: "var(--text)", fontWeight: 500 }}>{k}:</span> {v}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      </main>
    </div>
  );
}

function SectionHeader({ children, style }: { children: React.ReactNode; style?: React.CSSProperties }) {
  return (
    <div style={{
      fontSize: 10, fontWeight: 700, letterSpacing: 1,
      color: "var(--muted)", textTransform: "uppercase",
      marginBottom: 8, ...style,
    }}>
      {children}
    </div>
  );
}

function StatBlock({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{ fontSize: 10, color: "var(--muted)" }}>{label}</div>
      <div style={{ fontSize: 15, fontWeight: 600, color: color ?? "var(--text)", marginTop: 1 }}>{value}</div>
    </div>
  );
}

// ===========================================================================
// AUDIO TAB
// ===========================================================================
interface AudioTabProps {
  config: AppConfig;
  save:   (c: AppConfig) => Promise<void>;
}

const SEVERITY_META: Record<string, { label: string; desc: string; icon: string; defaultVol: number }> = {
  good: { label: "Good events",    desc: "Positive play — good cooldown usage, clean rotations",    icon: "✅", defaultVol: 0.6 },
  warn: { label: "Warning events", desc: "Suboptimal but recoverable — GCD gaps, minor delays",      icon: "⚠️", defaultVol: 0.7 },
  bad:  { label: "Error events",   desc: "Mistakes — avoidable damage, missed interrupts",           icon: "❌", defaultVol: 0.8 },
};

function AudioTab({ config, save }: AudioTabProps) {
  const cues: AudioCue[] = config.audio_cues ?? Object.entries(SEVERITY_META).map(([sev, m]) => ({
    severity: sev, enabled: true, volume: m.defaultVol, sound_path: "",
  }));

  function updateCue(severity: string, patch: Partial<AudioCue>) {
    void save({
      ...config,
      audio_cues: cues.map((c) => c.severity === severity ? { ...c, ...patch } : c),
    });
  }

  async function browseSoundFile(severity: string) {
    const selected = await open({
      filters: [{ name: "Audio Files", extensions: ["wav", "mp3", "ogg"] }],
      title:   `Select sound for ${severity} events`,
    });
    if (typeof selected === "string") updateCue(severity, { sound_path: selected });
  }

  function testBeep(severity: string, volume: number) {
    const ctx  = new AudioContext();
    const osc  = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.connect(gain);
    gain.connect(ctx.destination);
    gain.gain.value = volume;
    osc.frequency.value = severity === "good" ? 880 : severity === "warn" ? 660 : 440;
    osc.type = "sine";
    osc.start();
    osc.stop(ctx.currentTime + 0.2);
  }

  return (
    <div style={{ padding: 24, maxWidth: 640 }}>
      <h2 style={{ margin: "0 0 6px 0", fontSize: 16 }}>Audio Cues</h2>
      <p style={{ margin: "0 0 24px 0", fontSize: 12, color: "var(--muted)" }}>
        Play a sound when a coaching event fires. Each severity has its own volume and optional
        custom sound file. Leave the file blank to use the built-in tone.
        Audio cues work even when the overlay is hidden.
      </p>

      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        {(["good", "warn", "bad"] as const).map((sev) => {
          const cue  = cues.find((c) => c.severity === sev) ?? { severity: sev, enabled: true, volume: 0.7, sound_path: "" };
          const meta = SEVERITY_META[sev];
          return (
            <div key={sev} style={{
              background: "rgba(255,255,255,0.03)", border: "1px solid var(--stroke)",
              borderRadius: 10, padding: "14px 18px",
            }}>
              {/* Header row */}
              <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 12 }}>
                <span style={{ fontSize: 18, lineHeight: 1 }}>{meta.icon}</span>
                <div style={{ flex: 1 }}>
                  <div style={{ fontWeight: 600, fontSize: 13 }}>{meta.label}</div>
                  <div style={{ fontSize: 11, color: "var(--muted)" }}>{meta.desc}</div>
                </div>
                <label style={{ display: "flex", alignItems: "center", gap: 6, cursor: "pointer", margin: 0 }}>
                  <input
                    type="checkbox"
                    checked={cue.enabled}
                    onChange={(e) => updateCue(sev, { enabled: e.target.checked })}
                    style={{ width: "auto", accentColor: "var(--accent)", cursor: "pointer" }}
                  />
                  <span style={{ fontSize: 12 }}>Enabled</span>
                </label>
              </div>

              {/* Controls */}
              <div style={{
                display: "flex", flexDirection: "column", gap: 10,
                opacity: cue.enabled ? 1 : 0.35,
                pointerEvents: cue.enabled ? "auto" : "none",
              }}>
                {/* Volume row */}
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span style={{ fontSize: 11, color: "var(--muted)", minWidth: 48 }}>Volume</span>
                  <input
                    type="range" min={0} max={1} step={0.05}
                    value={cue.volume}
                    onChange={(e) => updateCue(sev, { volume: parseFloat(e.target.value) })}
                    style={{ flex: 1, padding: 0 }}
                  />
                  <span style={{ fontSize: 11, minWidth: 32, textAlign: "right" }}>
                    {Math.round(cue.volume * 100)}%
                  </span>
                  <button
                    style={{ fontSize: 11, padding: "4px 10px" }}
                    onClick={() => testBeep(sev, cue.volume)}
                  >
                    Test ▶
                  </button>
                </div>

                {/* Sound file row */}
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <span style={{ fontSize: 11, color: "var(--muted)", minWidth: 48 }}>File</span>
                  <div style={{
                    flex: 1, fontSize: 11,
                    color: cue.sound_path ? "var(--text)" : "var(--muted)",
                    fontStyle: cue.sound_path ? "normal" : "italic",
                    overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
                  }}>
                    {cue.sound_path || "Built-in tone"}
                  </div>
                  {cue.sound_path && (
                    <button style={{ fontSize: 11, padding: "4px 8px" }} onClick={() => updateCue(sev, { sound_path: "" })}>
                      ✕
                    </button>
                  )}
                  <button style={{ fontSize: 11, padding: "4px 10px" }} onClick={() => void browseSoundFile(sev)}>
                    Browse…
                  </button>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ===========================================================================
// HOTKEYS TAB
// ===========================================================================
interface HotkeysTabProps {
  config:        AppConfig;
  save:          (c: AppConfig) => Promise<void>;
  overlayOn:     boolean;
  toggleOverlay: () => void;
}

function HotkeysTab({ config, save, overlayOn, toggleOverlay }: HotkeysTabProps) {
  const [recording, setRecording] = useState(false);
  const [combo, setCombo]         = useState(config.hotkeys?.toggle_overlay ?? "");

  useEffect(() => {
    if (!recording) return;
    function onKey(e: KeyboardEvent) {
      e.preventDefault();
      const parts: string[] = [];
      if (e.ctrlKey)  parts.push("Ctrl");
      if (e.altKey)   parts.push("Alt");
      if (e.shiftKey) parts.push("Shift");
      const key = e.key;
      if (["Control", "Alt", "Shift", "Meta"].includes(key)) return;
      parts.push(key.length === 1 ? key.toUpperCase() : key);
      const newCombo = parts.join("+");
      setCombo(newCombo);
      setRecording(false);
      void save({ ...config, hotkeys: { ...(config.hotkeys ?? { toggle_overlay: "" }), toggle_overlay: newCombo } });
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [recording, config, save]);

  function clearHotkey() {
    setCombo("");
    void save({ ...config, hotkeys: { ...(config.hotkeys ?? { toggle_overlay: "" }), toggle_overlay: "" } });
  }

  return (
    <div style={{ padding: 24, maxWidth: 560 }}>
      <h2 style={{ margin: "0 0 6px 0", fontSize: 16 }}>Hotkeys</h2>
      <p style={{ margin: "0 0 24px 0", fontSize: 12, color: "var(--muted)" }}>
        Record keyboard shortcuts for quick control. Bindings are saved to config.
      </p>
      <div style={{
        background: "rgba(255,255,255,0.05)", border: "1px solid var(--warn)",
        borderRadius: 8, padding: "10px 14px", marginBottom: 24, fontSize: 12, color: "var(--warn)",
      }}>
        ⚠ Global hotkeys (active while WoW is in the foreground) require
        tauri-plugin-global-shortcut, planned for a future release.
        Use the <strong>Overlay ON/OFF</strong> button in the top bar in the meantime.
      </div>

      {/* Toggle overlay hotkey */}
      <div style={{
        background: "rgba(255,255,255,0.03)", border: "1px solid var(--stroke)",
        borderRadius: 10, padding: "16px 20px", marginBottom: 16,
      }}>
        <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 4 }}>Toggle Overlay</div>
        <div style={{ fontSize: 11, color: "var(--muted)", marginBottom: 14 }}>
          Show or hide the in-game overlay.
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <div style={{
            flex: 1, padding: "8px 12px", borderRadius: 8,
            border: recording ? "1px solid var(--accent)" : "1px solid var(--stroke)",
            background: recording ? "rgba(124,92,255,0.1)" : "rgba(11,15,24,0.65)",
            fontSize: 13, fontFamily: "var(--mono)",
            color: combo ? "var(--text)" : "var(--muted)",
            fontStyle: combo ? "normal" : "italic",
            minHeight: 36, display: "flex", alignItems: "center",
          }}>
            {recording ? "Press your key combo…" : (combo || "Not set")}
          </div>
          <button
            onClick={() => setRecording((r) => !r)}
            className={recording ? "primary" : ""}
          >
            {recording ? "Cancel" : "Record"}
          </button>
          {combo && <button onClick={clearHotkey}>Clear</button>}
        </div>
      </div>

      {/* Quick-action */}
      <div style={{
        background: "rgba(255,255,255,0.03)", border: "1px solid var(--stroke)",
        borderRadius: 10, padding: "16px 20px",
      }}>
        <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 4 }}>Quick Actions</div>
        <div style={{ fontSize: 11, color: "var(--muted)", marginBottom: 14 }}>
          Immediate effect — mirrors the top bar toggle.
        </div>
        <button
          onClick={toggleOverlay}
          className={overlayOn ? "primary" : ""}
          style={{ fontSize: 13, padding: "10px 20px" }}
        >
          {overlayOn ? "🟢 Overlay ON — click to hide" : "🔴 Overlay OFF — click to show"}
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Error Boundary — catches render errors and shows them instead of blank
// ---------------------------------------------------------------------------
interface EBState { error: Error | null }
class ErrorBoundary extends React.Component<{ children: React.ReactNode }, EBState> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { error: null };
  }
  static getDerivedStateFromError(error: Error): EBState { return { error }; }
  override componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("[ErrorBoundary]", error, info);
  }
  override render() {
    if (this.state.error) {
      return (
        <div style={{
          padding: 32, color: "#ff5c77", fontFamily: "monospace",
          background: "#0b0f18", minHeight: "100vh",
        }}>
          <div style={{ fontSize: 16, fontWeight: 700, marginBottom: 12 }}>
            ⚠ Render error — please screenshot this and report it
          </div>
          <div style={{ fontSize: 13, marginBottom: 8 }}>
            {this.state.error.message}
          </div>
          <pre style={{ fontSize: 11, color: "#a9b6d6", whiteSpace: "pre-wrap", overflow: "auto" }}>
            {this.state.error.stack}
          </pre>
        </div>
      );
    }
    return this.props.children;
  }
}

// Catch errors that happen outside the React tree (module load, async, etc.)
window.addEventListener("error", (e) => {
  const root = document.getElementById("root");
  if (root && root.childNodes.length === 0) {
    root.innerHTML = `<div style="padding:32px;color:#ff5c77;font-family:monospace;background:#0b0f18;min-height:100vh">
      <div style="font-size:16px;font-weight:700;margin-bottom:12px">⚠ Uncaught error — please screenshot this</div>
      <div style="font-size:13px;margin-bottom:8px">${e.message}</div>
      <pre style="font-size:11px;color:#a9b6d6;white-space:pre-wrap">${e.filename}:${e.lineno}\n${e.error?.stack ?? ""}</pre>
    </div>`;
  }
});

window.addEventListener("unhandledrejection", (e) => {
  console.error("[unhandledrejection]", e.reason);
  const root = document.getElementById("root");
  if (root && root.childNodes.length === 0) {
    root.innerHTML = `<div style="padding:32px;color:#ff5c77;font-family:monospace;background:#0b0f18;min-height:100vh">
      <div style="font-size:16px;font-weight:700;margin-bottom:12px">⚠ Unhandled Promise rejection — please screenshot this</div>
      <pre style="font-size:11px;color:#a9b6d6;white-space:pre-wrap">${String(e.reason)}</pre>
    </div>`;
  }
});

createRoot(document.getElementById("root")!).render(
  <ErrorBoundary>
    <SettingsApp />
  </ErrorBoundary>
);
