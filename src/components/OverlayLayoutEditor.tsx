// Overlay layout editor — lives in the SETTINGS window only.
//
// Shows a scaled-down preview canvas (30% of 1920×1080) with static panel
// handles and per-panel controls for X, Y, visibility, opacity, and scale.
//
// @dnd-kit intentionally removed — it bundled a second copy of React which
// caused React error #310 (invalid hook call) in the production build.
// Drag-and-drop will be re-added in a future release once the dependency
// conflict is resolved. For now X/Y are set via number inputs.
import type { PanelPosition } from "../types/events";
import styles from "./OverlayLayoutEditor.module.css";

// Scale: editor canvas width / real screen width
// Default to 1920×1080; overridden by the screenWidth/screenHeight props when
// the parent component has fetched the actual overlay monitor dimensions.
const PREVIEW_CANVAS_PX = 576; // editor canvas is always ~576 px wide

const PANEL_LABELS: Record<string, string> = {
  pull_clock:   "Pull Clock",
  now_feed:     "Now Feed",
  timeline:     "Timeline",
  stat_widgets: "Stats",
};

const PANEL_COLORS: Record<string, string> = {
  pull_clock:   "rgba(124, 92,255,0.55)",
  now_feed:     "rgba( 43,213,118,0.45)",
  timeline:     "rgba(255,204,102,0.45)",
  stat_widgets: "rgba(255, 92,119,0.45)",
};

interface Props {
  positions:        PanelPosition[];
  onPositionChange: (updated: PanelPosition[]) => void;
  /** Actual overlay monitor width in physical pixels (default 1920). */
  screenWidth?:     number;
  /** Actual overlay monitor height in physical pixels (default 1080). */
  screenHeight?:    number;
}

export function OverlayLayoutEditor({ positions, onPositionChange, screenWidth = 1920, screenHeight = 1080 }: Props) {
  // Compute scale so the preview canvas is always PREVIEW_CANVAS_PX wide.
  const scale = PREVIEW_CANVAS_PX / screenWidth;
  const W     = PREVIEW_CANVAS_PX;
  const H     = Math.round(screenHeight * scale);

  function patch(id: string, changes: Partial<PanelPosition>) {
    onPositionChange(
      positions.map((p) => (p.id === id ? { ...p, ...changes } : p))
    );
  }

  return (
    <div className={styles.wrap}>
      {/* ── Preview canvas ── */}
      <div className={styles.hint}>
        Preview ({Math.round(scale * 100)}% scale — {W} × {H} px represents {screenWidth} × {screenHeight}).
        Set exact pixel positions using the X / Y inputs below.
      </div>

      <div className={styles.canvas} style={{ width: W, height: H }}>
        {positions.map((p) => {
          const left = Math.min(Math.max(0, p.x * scale), W - 4);
          const top  = Math.min(Math.max(0, p.y * scale), H - 4);
          return (
            <div
              key={p.id}
              className={styles.handle}
              style={{
                left,
                top,
                opacity:    p.visible ? 1 : 0.3,
                background: PANEL_COLORS[p.id] ?? "rgba(124,92,255,0.45)",
                transform:  `scale(${p.scale ?? 1.0})`,
                transformOrigin: "top left",
              }}
            >
              {PANEL_LABELS[p.id] ?? p.id}
            </div>
          );
        })}
      </div>

      {/* ── Per-panel controls ── */}
      <div className={styles.panelControls}>
        {positions.map((p) => (
          <div key={p.id} className={styles.panelRow}>
            {/* Label + visibility toggle */}
            <div className={styles.panelRowHeader}>
              <span className={styles.panelRowLabel}>{PANEL_LABELS[p.id] ?? p.id}</span>
              <button
                className={styles.eye}
                onClick={() => patch(p.id, { visible: !p.visible })}
                title={p.visible ? "Hide panel" : "Show panel"}
              >
                {p.visible ? "👁" : "🚫"}
              </button>
            </div>

            {/* X / Y position inputs */}
            <div className={styles.xyRow}>
              <label className={styles.xyLabel}>
                X
                <input
                  type="number"
                  min={0}
                  max={screenWidth}
                  step={10}
                  value={p.x}
                  onChange={(e) => patch(p.id, { x: Math.max(0, Number(e.target.value)) })}
                  className={styles.xyInput}
                />
              </label>
              <label className={styles.xyLabel}>
                Y
                <input
                  type="number"
                  min={0}
                  max={screenHeight}
                  step={10}
                  value={p.y}
                  onChange={(e) => patch(p.id, { y: Math.max(0, Number(e.target.value)) })}
                  className={styles.xyInput}
                />
              </label>
            </div>

            {/* Opacity slider */}
            <label className={styles.sliderLabel}>
              Opacity
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={p.opacity ?? 1.0}
                onChange={(e) => patch(p.id, { opacity: parseFloat(e.target.value) })}
                className={styles.slider}
              />
              <span className={styles.sliderValue}>
                {Math.round((p.opacity ?? 1.0) * 100)}%
              </span>
            </label>

            {/* Scale slider */}
            <label className={styles.sliderLabel}>
              Scale
              <input
                type="range"
                min={0.5}
                max={2.0}
                step={0.05}
                value={p.scale ?? 1.0}
                onChange={(e) => patch(p.id, { scale: parseFloat(e.target.value) })}
                className={styles.slider}
              />
              <span className={styles.sliderValue}>
                {((p.scale ?? 1.0) * 100).toFixed(0)}%
              </span>
            </label>
          </div>
        ))}
      </div>
    </div>
  );
}
