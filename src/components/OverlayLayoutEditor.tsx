// Drag-and-drop overlay layout editor — lives in the SETTINGS window only.
//
// Renders a scaled-down preview canvas (30% of 1920×1080) with draggable
// panel handles. The user positions panels here; positions are saved to
// AppConfig and the overlay reads them on next load.
//
// Uses @dnd-kit/core for drag behavior.
import {
  DndContext,
  useDraggable,
  type DragEndEvent,
} from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import type { PanelPosition } from "../types/events";
import styles from "./OverlayLayoutEditor.module.css";

// Scale: editor canvas width / real screen width
const SCALE  = 0.30;
const W      = Math.round(1920 * SCALE); // 576px
const H      = Math.round(1080 * SCALE); // 324px

const PANEL_LABELS: Record<string, string> = {
  pull_clock:   "Pull Clock",
  now_feed:     "Now Feed",
  timeline:     "Timeline",
  stat_widgets: "Stats",
};

interface Props {
  positions:        PanelPosition[];
  onPositionChange: (updated: PanelPosition[]) => void;
}

export function OverlayLayoutEditor({ positions, onPositionChange }: Props) {
  function handleDragEnd(e: DragEndEvent) {
    const { active, delta } = e;
    const id = active.id as string;

    onPositionChange(
      positions.map((p) => {
        if (p.id !== id) return p;
        // Convert scaled pixel delta back to real screen coordinates
        return {
          ...p,
          x: Math.max(0, Math.round(p.x + delta.x / SCALE)),
          y: Math.max(0, Math.round(p.y + delta.y / SCALE)),
        };
      })
    );
  }

  function toggleVisible(id: string) {
    onPositionChange(
      positions.map((p) => (p.id === id ? { ...p, visible: !p.visible } : p))
    );
  }

  return (
    <div className={styles.wrap}>
      <div className={styles.hint}>
        Drag panels to reposition. Click the eye icon to show/hide.
      </div>
      <DndContext onDragEnd={handleDragEnd}>
        <div className={styles.canvas} style={{ width: W, height: H }}>
          {positions.map((p) => (
            <DraggableHandle
              key={p.id}
              position={p}
              scale={SCALE}
              label={PANEL_LABELS[p.id] ?? p.id}
              onToggleVisible={() => toggleVisible(p.id)}
            />
          ))}
        </div>
      </DndContext>
    </div>
  );
}

// ---------------------------------------------------------------------------

interface HandleProps {
  position:        PanelPosition;
  scale:           number;
  label:           string;
  onToggleVisible: () => void;
}

function DraggableHandle({ position, scale, label, onToggleVisible }: HandleProps) {
  const { attributes, listeners, setNodeRef, transform } = useDraggable({
    id: position.id,
  });

  const style: React.CSSProperties = {
    position:  "absolute",
    left:      position.x * scale,
    top:       position.y * scale,
    transform: CSS.Translate.toString(transform),
    opacity:   position.visible ? 1 : 0.4,
  };

  return (
    <div ref={setNodeRef} style={style} className={styles.handle}>
      <div className={styles.grip} {...listeners} {...attributes}>
        ⠿
      </div>
      <span className={styles.handleLabel}>{label}</span>
      <button
        className={styles.eye}
        onClick={onToggleVisible}
        title={position.visible ? "Hide panel" : "Show panel"}
      >
        {position.visible ? "👁" : "🚫"}
      </button>
    </div>
  );
}
