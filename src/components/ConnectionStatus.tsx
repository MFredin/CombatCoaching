// Shows two status indicators: log tailing and addon handshake.
import type { ConnectionStatus as Status } from "../types/events";
import styles from "./ConnectionStatus.module.css";

interface Props {
  status: Status;
}

export function ConnectionStatus({ status }: Props) {
  return (
    <div className={styles.wrap}>
      <Pill label="LOG"    on={status.log_tailing}     />
      <Pill label="ADDON"  on={status.addon_connected}  />
    </div>
  );
}

function Pill({ label, on }: { label: string; on: boolean }) {
  return (
    <div className={`${styles.pill} ${on ? styles.on : ""}`}>
      <span className={styles.led} />
      <span className={styles.label}>{label}</span>
      <span className={styles.state}>{on ? "Connected" : "Disconnected"}</span>
    </div>
  );
}
