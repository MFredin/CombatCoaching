// 30-second event timeline — vertical ticks coloured by severity.
// Ticks are positioned horizontally by recency (newest = right).
import { useMemo } from "react";
import type { AdviceEvent } from "../types/events";
import styles from "./Timeline.module.css";

interface Props {
  advice:   AdviceEvent[];
  windowMs: number; // e.g. 30_000
}

export function Timeline({ advice, windowMs }: Props) {
  const now = Date.now();

  const ticks = useMemo(() => {
    return advice
      .filter((a) => now - a.timestamp_ms < windowMs)
      .map((a) => ({
        key:    a.key + a.timestamp_ms,
        xPct:   ((a.timestamp_ms - (now - windowMs)) / windowMs) * 100,
        height: a.severity === "bad" ? 80 : a.severity === "warn" ? 55 : 35,
        sev:    a.severity,
      }));
  }, [advice, windowMs, now]);

  return (
    <div className={styles.wrap}>
      <div className={styles.label}>Events (last 30s) →</div>
      {ticks.map((t) => (
        <div
          key={t.key}
          className={`${styles.tick} ${styles[t.sev]}`}
          style={{ left: `${t.xPct}%`, height: t.height }}
        />
      ))}
    </div>
  );
}
