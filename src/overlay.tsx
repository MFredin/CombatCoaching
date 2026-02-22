// Entry point for the transparent overlay window.
// Renders absolutely-positioned coaching panels using stored config positions.
// This window is always-on-top and click-through (set_ignore_cursor_events on Rust side).
// Layout editing happens in the SETTINGS window — this just reads saved positions.
import React, { useState, useEffect, useCallback, useRef } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { NowFeed }           from "./components/NowFeed";
import { Timeline }          from "./components/Timeline";
import { StatWidgets }       from "./components/StatWidgets";
import { PullClock }         from "./components/PullClock";
import { PullDebriefPanel }  from "./components/PullDebriefPanel";
import { useTauriEvents }    from "./hooks/useTauriEvents";
import type {
  AdviceEvent,
  AudioCue,
  PullDebrief,
  StateSnapshot,
  AppConfig,
  PanelPosition,
} from "./types/events";
import "./styles/overlay.css";

const MAX_CARDS  = 3;
const CARD_TTL   = 30_000; // ms — cards vanish after 30 seconds
const WINDOW_MS  = 30_000;

// ---------------------------------------------------------------------------
// Audio cue playback — Web Audio API, no external dependencies.
// A single AudioContext is reused across calls to avoid the browser's
// per-page context limit. Resume is called before each beep because the
// context may be suspended until the first user gesture (or auto-resumed
// by the WebView2 desktop app context).
// ---------------------------------------------------------------------------
let _audioCtx: AudioContext | null = null;

function getAudioCtx(): AudioContext {
  if (!_audioCtx || _audioCtx.state === "closed") {
    _audioCtx = new AudioContext();
  }
  return _audioCtx;
}

function playAudioCue(severity: string, cues: AudioCue[]): void {
  const cue = cues.find((c) => c.severity === severity);
  if (!cue?.enabled) return;

  try {
    const ctx = getAudioCtx();
    const play = () => {
      const osc  = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.connect(gain);
      gain.connect(ctx.destination);
      gain.gain.value     = Math.max(0, Math.min(1, cue.volume));
      osc.frequency.value = severity === "good" ? 880 : severity === "warn" ? 660 : 440;
      osc.type            = "sine";
      osc.start();
      osc.stop(ctx.currentTime + 0.15);
      osc.onended = () => {};
    };
    // Resume context if suspended (WebView2 may suspend until first interaction)
    if (ctx.state === "suspended") {
      ctx.resume().then(play).catch(() => {});
    } else {
      play();
    }
  } catch {
    // Audio not available — silently ignore
  }
}

// ---------------------------------------------------------------------------
// Overlay app
// ---------------------------------------------------------------------------

function OverlayApp() {
  const [advice, setAdvice]     = useState<AdviceEvent[]>([]);
  const [snapshot, setSnapshot] = useState<StateSnapshot>({
    pull_elapsed_ms: 0,
    gcd_gap_ms:      0,
    avoidable_count: 0,
    in_combat:       false,
    interrupt_count: 0,
    encounter_name:  null,
  });
  const [debrief, setDebrief]   = useState<PullDebrief | null>(null);
  const [panels, setPanels]     = useState<PanelPosition[]>([]);
  // Audio cues kept in a ref — no re-renders needed when config reloads
  const audioCuesRef = useRef<AudioCue[]>([]);

  // Load panel positions and audio config from config on mount
  useEffect(() => {
    invoke<AppConfig>("get_config")
      .then((cfg) => {
        setPanels(cfg.panel_positions ?? []);
        audioCuesRef.current = cfg.audio_cues ?? [];
      })
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
      // Play audio cue for this severity
      playAudioCue(incoming.severity, audioCuesRef.current);
    }, []),

    onStateSnapshot: useCallback((snap: StateSnapshot) => {
      setSnapshot(snap);
    }, []),

    onDebrief: useCallback((d: PullDebrief) => {
      setDebrief(d);
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
        <PullClock
          elapsedMs={snapshot.pull_elapsed_ms}
          inCombat={snapshot.in_combat}
          encounterName={snapshot.encounter_name}
        />
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
          interruptCount={snapshot.interrupt_count}
        />
      </AbsPanel>

      {/* Debrief panel — auto-positioned bottom-right-ish, auto-dismisses */}
      <AbsPanel pos={pos("debrief") ?? { id: "debrief", x: 20, y: 400, visible: true }}>
        <PullDebriefPanel debrief={debrief} />
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
