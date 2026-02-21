// Pull timer — shows MM:SS elapsed since pull start.
// Shows "-- : --" when not in combat.
import styles from "./PullClock.module.css";

interface Props {
  elapsedMs: number;
  inCombat:  boolean;
}

function fmt(ms: number): string {
  const totalS = Math.floor(ms / 1000);
  const m = Math.floor(totalS / 60);
  const s = totalS % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function PullClock({ elapsedMs, inCombat }: Props) {
  return (
    <div className={`${styles.clock} ${inCombat ? styles.active : ""}`}>
      <span className={styles.label}>PULL</span>
      <span className={styles.time}>{inCombat ? fmt(elapsedMs) : "-- : --"}</span>
    </div>
  );
}
