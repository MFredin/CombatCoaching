// Three spark stat boxes: GCD gap, avoidable hits, and interrupts.
import styles from "./StatWidgets.module.css";

interface Props {
  gcdGapMs:       number;
  avoidableCount: number;
  interruptCount: number;
}

export function StatWidgets({ gcdGapMs, avoidableCount, interruptCount }: Props) {
  const gapS    = (gcdGapMs / 1000).toFixed(1);
  const gapBad  = gcdGapMs >= 2500;
  const hitsBad = avoidableCount >= 2;
  const intGood = interruptCount > 0;

  return (
    <div className={styles.row}>
      <div className={`${styles.spark} ${gapBad ? styles.warn : ""}`}>
        <div className={styles.big}>{gapS}s</div>
        <div className={styles.small}>GCD gap</div>
      </div>
      <div className={`${styles.spark} ${hitsBad ? styles.bad : ""}`}>
        <div className={styles.big}>{avoidableCount}</div>
        <div className={styles.small}>Avoidable hits</div>
      </div>
      <div className={`${styles.spark} ${intGood ? styles.good : ""}`}>
        <div className={styles.big}>{interruptCount}</div>
        <div className={styles.small}>Interrupts</div>
      </div>
    </div>
  );
}
