// Entry point for the settings window.
import { useState, useEffect, useCallback } from "react";
import { createRoot } from "react-dom/client";
import { invoke }     from "@tauri-apps/api/core";
import { open }       from "@tauri-apps/plugin-dialog";
import { ConnectionStatus }      from "./components/ConnectionStatus";
import { OverlayLayoutEditor }   from "./components/OverlayLayoutEditor";
import { useTauriEvents }        from "./hooks/useTauriEvents";
import type {
  AppConfig,
  ConnectionStatus as ConnStatus,
  PanelPosition,
} from "./types/events";
import "./styles/settings.css";

// ---------------------------------------------------------------------------
// Root settings app
// ---------------------------------------------------------------------------

function SettingsApp() {
  const [config, setConfig]         = useState<AppConfig | null>(null);
  const [connStatus, setConnStatus] = useState<ConnStatus>({
    log_tailing: false, addon_connected: false, wow_path: "",
  });
  const [saving, setSaving]         = useState(false);
  const [detectMsg, setDetectMsg]   = useState("");

  // Load config on mount
  useEffect(() => {
    invoke<AppConfig>("get_config").then(setConfig).catch(console.error);
  }, []);

  useTauriEvents({
    onConnection: useCallback((s: ConnStatus) => setConnStatus(s), []),
  });

  if (!config) {
    return <div style={{ padding: 32, color: "var(--muted)" }}>Loading…</div>;
  }

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
      const updated = { ...config, wow_log_path: found };
      await save(updated);
      setDetectMsg(`Found: ${found}`);
    } else {
      setDetectMsg("Not found automatically. Browse to your WoW Logs folder.");
    }
  }

  async function browsePath() {
    const selected = await open({
      directory: true,
      title:     "Select WoW Logs Folder (e.g. …\\World of Warcraft\\_retail_\\Logs)",
    });
    if (typeof selected === "string") {
      await save({ ...config, wow_log_path: selected });
    }
  }

  async function browseAddonPath() {
    const selected = await open({
      filters: [{ name: "Lua Saved Variables", extensions: ["lua"] }],
      title:   "Select CombatCoach.lua SavedVariables",
    });
    if (typeof selected === "string") {
      await save({ ...config, addon_sv_path: selected });
    }
  }

  function updatePanels(positions: PanelPosition[]) {
    const updated = { ...config, panel_positions: positions };
    setConfig(updated);
    void save(updated);
  }

  const intensityLabels: Record<number, string> = {
    1: "1 — Quiet: critical mistakes only",
    2: "2 — Low: major mistakes + big wins",
    3: "3 — Balanced: clear-value callouts",
    4: "4 — High: include uptime gaps + interrupts",
    5: "5 — Maximum: aggressive, frequent",
  };

  return (
    <div style={{ display: "grid", gridTemplateColumns: "260px 1fr", height: "100vh" }}>
      {/* Left sidebar */}
      <aside style={{ borderRight: "1px solid var(--stroke)", overflow: "auto" }}>
        <div style={{ padding: "16px", borderBottom: "1px solid var(--stroke)" }}>
          <div style={{ fontWeight: 700, fontSize: 14 }}>CombatLedger</div>
          <div style={{ fontSize: 11, color: "var(--muted)", marginTop: 2 }}>Live Coach v0.1</div>
        </div>
        <div className="section">
          <h3>Connection</h3>
          <ConnectionStatus status={connStatus} />
        </div>
        <div className="section">
          <h3>WoW Logs Folder</h3>
          <div style={{ fontSize: 11, color: "var(--muted)", wordBreak: "break-all", marginBottom: 4 }}>
            {config.wow_log_path || "Not configured"}
          </div>
          <div style={{ fontSize: 10, color: "var(--muted)", marginBottom: 8, fontStyle: "italic" }}>
            The newest WoWCombatLog*.txt in this folder is tailed automatically.
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
          <h3>Addon SavedVariables</h3>
          <div style={{ fontSize: 11, color: "var(--muted)", wordBreak: "break-all", marginBottom: 8 }}>
            {config.addon_sv_path || "Not configured"}
          </div>
          <button onClick={browseAddonPath}>Browse…</button>
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
      </aside>

      {/* Main content */}
      <main style={{ overflow: "auto", padding: 24 }}>
        <h2 style={{ margin: "0 0 6px 0", fontSize: 16 }}>Overlay Layout</h2>
        <p style={{ margin: "0 0 16px 0", fontSize: 12, color: "var(--muted)" }}>
          Drag panels to set their position on your screen. Changes take effect after restarting the overlay or relaunching the app.
        </p>
        <OverlayLayoutEditor
          positions={config.panel_positions ?? []}
          onPositionChange={updatePanels}
        />
        {saving && (
          <div style={{ marginTop: 12, fontSize: 11, color: "var(--muted)" }}>Saving…</div>
        )}
      </main>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<SettingsApp />);
