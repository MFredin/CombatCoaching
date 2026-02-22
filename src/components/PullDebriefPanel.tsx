// End-of-pull debrief panel — shows a quick summary on the overlay after each pull.
// Auto-dismisses after DISPLAY_MS. Replaces itself immediately if a new pull ends.
//
// Shows:
//   - Pull # and outcome (Kill / Wipe)
//   - Pull duration (MM:SS)
//   - Avoidable hits, interrupts, total advice fired, GCD gap events
import React, { useEffect, useState } from "react";
import type { PullDebrief } from "../types/events";
import styles from "./PullDebriefPanel.module.css";

const DISPLAY_MS = 10_000; // auto-dismiss after 10 seconds

interface Props {
  debrief: PullDebrief | null;
}

function fmtElapsed(ms: number): string {
  const totalS = Math.floor(ms / 1000);
  const m = Math.floor(totalS / 60);
  const s = totalS % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function PullDebriefPanel({ debrief }: Props) {
  const [visible, setVisible] = useState(false);

  // Show on new debrief, auto-hide after DISPLAY_MS
  useEffect(() => {
    if (!debrief) return;
    setVisible(true);
    const id = setTimeout(() => setVisible(false), DISPLAY_MS);
    return () => clearTimeout(id);
  }, [debrief]);

  if (!visible || !debrief) return null;

  const isKill    = debrief.outcome === "kill";
  const isWipe    = debrief.outcome === "wipe";
  const outcomeColor = isKill ? "var(--good)" : isWipe ? "var(--bad)" : "var(--muted)";
  const outcomeLabel = isKill ? "KILL" : isWipe ? "WIPE" : "UNKNOWN";

  const rows: { label: string; value: string; color?: string }[] = [
    { label: "Pull time",     value: fmtElapsed(debrief.pull_elapsed_ms) },
    {
      label: "Avoidable hits",
      value: debrief.avoidable_count.toString(),
      color: debrief.avoidable_count > 0 ? "var(--bad)" : "var(--good)",
    },
    {
      label: "Interrupts",
      value: debrief.interrupt_count.toString(),
      color: debrief.interrupt_count > 0 ? "var(--good)" : undefined,
    },
    { label: "GCD gaps",      value: debrief.gcd_gap_count.toString(),
      color: debrief.gcd_gap_count > 0 ? "var(--warn)" : undefined },
    { label: "Advice fired",  value: debrief.total_advice_fired.toString() },
  ];

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <span className={styles.pullNum}>Pull #{debrief.pull_number}</span>
        <span className={styles.outcome} style={{ color: outcomeColor }}>
          {outcomeLabel}
        </span>
      </div>

      <div className={styles.grid}>
        {rows.map(({ label, value, color }) => (
          <React.Fragment key={label}>
            <span className={styles.label}>{label}</span>
            <span className={styles.value} style={{ color: color ?? "var(--text)" }}>
              {value}
            </span>
          </React.Fragment>
        ))}
      </div>

      <div className={styles.footer}>
        Dismisses automatically
      </div>
    </div>
  );
}
