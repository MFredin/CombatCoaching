// Pull timer — shows MM:SS elapsed since pull start.
// Shows "-- : --" when not in combat.
// Optionally shows the active encounter name below the timer.
import styles from "./PullClock.module.css";

interface Props {
  elapsedMs:      number;
  inCombat:       boolean;
  encounterName?: string | null;
}

function fmt(ms: number): string {
  const totalS = Math.floor(ms / 1000);
  const m = Math.floor(totalS / 60);
  const s = totalS % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function PullClock({ elapsedMs, inCombat, encounterName }: Props) {
  return (
    <div className={`${styles.clock} ${inCombat ? styles.active : ""}`}>
      <div className={styles.row}>
        <span className={styles.label}>PULL</span>
        <span className={styles.time}>{inCombat ? fmt(elapsedMs) : "-- : --"}</span>
      </div>
      {encounterName && (
        <div className={styles.encounter}>{encounterName}</div>
      )}
    </div>
  );
}
