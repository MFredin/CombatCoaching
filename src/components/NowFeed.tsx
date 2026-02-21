// The 1–3 prioritized coaching cards panel.
// Matches the .now / .alert / .sev design from the mockup exactly.
import type { AdviceEvent } from "../types/events";
import styles from "./NowFeed.module.css";

interface Props {
  advice: AdviceEvent[];
}

export function NowFeed({ advice }: Props) {
  if (advice.length === 0) {
    return (
      <div className={styles.feed}>
        <div className={styles.empty}>Waiting for combat events…</div>
      </div>
    );
  }

  return (
    <div className={styles.feed}>
      {advice.map((a) => (
        <div key={a.key} className={styles.card}>
          <div className={`${styles.sev} ${styles[a.severity]}`} />
          <div className={styles.body}>
            <div className={styles.title}>{a.title}</div>
            <div className={styles.message}>{a.message}</div>
            {a.kv.length > 0 && (
              <div className={styles.kvs}>
                {a.kv.map(([k, v]) => (
                  <span key={k} className={styles.kv}>
                    {k}={v}
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}
