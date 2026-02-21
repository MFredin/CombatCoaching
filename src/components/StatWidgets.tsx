// Two spark stat boxes: GCD gap and avoidable hit count.
import styles from "./StatWidgets.module.css";

interface Props {
  gcdGapMs:       number;
  avoidableCount: number;
}

export function StatWidgets({ gcdGapMs, avoidableCount }: Props) {
  const gapS    = (gcdGapMs / 1000).toFixed(1);
  const gapBad  = gcdGapMs >= 2500;
  const hitsBad = avoidableCount >= 2;

  return (
    <div className={styles.row}>
      <div className={`${styles.spark} ${gapBad ? styles.warn : ""}`}>
        <div className={styles.big}>{gapS}s</div>
        <div className={styles.small}>GCD gap (last cast)</div>
      </div>
      <div className={`${styles.spark} ${hitsBad ? styles.bad : ""}`}>
        <div className={styles.big}>{avoidableCount}</div>
        <div className={styles.small}>Avoidable hits this pull</div>
      </div>
    </div>
  );
}
