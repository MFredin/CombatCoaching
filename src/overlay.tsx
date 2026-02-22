// Entry point for the transparent overlay window.
// Renders absolutely-positioned coaching panels using stored config positions.
// This window is always-on-top and click-through (set_ignore_cursor_events on Rust side).
// Layout editing happens in the SETTINGS window — this just reads saved positions.
import React, { useState, useEffect, useCallback } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { NowFeed }      from "./components/NowFeed";
import { Timeline }     from "./components/Timeline";
import { StatWidgets }  from "./components/StatWidgets";
import { PullClock }    from "./components/PullClock";
import { useTauriEvents } from "./hooks/useTauriEvents";
import type {
  AdviceEvent,
  StateSnapshot,
  AppConfig,
  PanelPosition,
} from "./types/events";
import "./styles/overlay.css";

const MAX_CARDS  = 3;
const CARD_TTL   = 30_000; // ms — cards vanish after 30 seconds
const WINDOW_MS  = 30_000;

function OverlayApp() {
  const [advice, setAdvice]         = useState<AdviceEvent[]>([]);
  const [snapshot, setSnapshot]     = useState<StateSnapshot>({
    pull_elapsed_ms: 0,
    gcd_gap_ms:      0,
    avoidable_count: 0,
    in_combat:       false,
  });
  const [panels, setPanels]         = useState<PanelPosition[]>([]);

  // Load panel positions from config on mount
  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then((cfg) => setPanels(cfg.panel_positions ?? []))
      .catch(() => {}); // No config yet — panels use default positions
  }, []);

  // Subscribe to backend IPC events
  useTauriEvents({
    onAdvice: useCallback((incoming: AdviceEvent) => {
      setAdvice((prev) => {
        // Replace card with same key (dedup), prepend new card, cap at MAX_CARDS
        const filtered = prev.filter((a) => a.key !== incoming.key);
        return [incoming, ...filtered].slice(0, MAX_CARDS);
      });
    }, []),

    onStateSnapshot: useCallback((snap: StateSnapshot) => {
      setSnapshot(snap);
    }, []),
  });

  // Expire stale cards
  useEffect(() => {
    const id = setInterval(() => {
      const cutoff = Date.now() - CARD_TTL;
      setAdvice((prev) => prev.filter((a) => a.timestamp_ms > cutoff));
    }, 1_000);
    return () => clearInterval(id);
  }, []);

  const pos = (id: string): PanelPosition =>
    panels.find((p) => p.id === id) ?? { id, x: 20, y: 80, visible: true };

  return (
    <>
      <AbsPanel pos={pos("pull_clock")}>
        <PullClock elapsedMs={snapshot.pull_elapsed_ms} inCombat={snapshot.in_combat} />
      </AbsPanel>

      <AbsPanel pos={pos("now_feed")}>
        <NowFeed advice={advice} />
      </AbsPanel>

      <AbsPanel pos={pos("timeline")}>
        <Timeline advice={advice} windowMs={WINDOW_MS} />
      </AbsPanel>

      <AbsPanel pos={pos("stat_widgets")}>
        <StatWidgets
          gcdGapMs={snapshot.gcd_gap_ms}
          avoidableCount={snapshot.avoidable_count}
        />
      </AbsPanel>
    </>
  );
}

function AbsPanel({
  pos,
  children,
}: {
  pos: PanelPosition;
  children: React.ReactNode;
}) {
  if (!pos.visible) return null;
  const opacity = pos.opacity ?? 1.0;
  const scale   = pos.scale   ?? 1.0;
  return (
    <div
      style={{
        position:        "absolute",
        left:            pos.x,
        top:             pos.y,
        opacity,
        transform:       scale !== 1.0 ? `scale(${scale})` : undefined,
        transformOrigin: "top left",
        pointerEvents:   "none", // All overlay elements are non-interactive
      }}
    >
      {children}
    </div>
  );
}

createRoot(document.getElementById("overlay-root")!).render(<OverlayApp />);
