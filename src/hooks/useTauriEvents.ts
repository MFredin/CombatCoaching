// Polls Tauri backend commands for all event data.
//
// All listen()-based event delivery has been replaced with invoke() polling
// because capabilities files cause a startup crash (0xc0000409) in this
// codebase due to a tauri-build (2.5.5) / tauri runtime (2.10.2) version
// mismatch.  Without a capabilities file, plugin:event|listen is denied by
// the ACL.  invoke() for user-defined #[tauri::command] functions always works
// without any capabilities file — confirmed working since v0.9.7.
//
// Poll intervals:
//   get_connection_status  — every 1000 ms  (connection health pill)
//   get_state_snapshot     — every  300 ms  (pull clock, stat widgets, timeline)
//   drain_advice_queue     — every  500 ms  (advice cards in Live Feed + overlay)
//
// Handlers are stored in a ref so the effect closure never goes stale when
// the parent component re-renders.  Intervals are created once on mount and
// cleared on unmount.
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  type AdviceEvent,
  type StateSnapshot,
  type ConnectionStatus,
  type PlayerIdentity,
  type PullDebrief,
} from "../types/events";

export interface TauriEventHandlers {
  onAdvice?:        (event: AdviceEvent)       => void;
  onStateSnapshot?: (snapshot: StateSnapshot)  => void;
  onConnection?:    (status: ConnectionStatus) => void;
  onIdentity?:      (identity: PlayerIdentity) => void;
  onDebrief?:       (debrief: PullDebrief)     => void;
  /** Called with a batch of new event log entries (e.g. "12:34:56 ⚠️ GCD Gap"). */
  onEventLog?:      (entries: string[])        => void;
}

export function useTauriEvents(handlers: TauriEventHandlers): void {
  // Keep latest handlers in a ref so the effect closure never goes stale
  const ref = useRef(handlers);
  ref.current = handlers;

  useEffect(() => {
    const intervals: ReturnType<typeof setInterval>[] = [];

    // ── Connection status ────────────────────────────────────────────────────
    // Polled every 1 s regardless of any event system status.
    // invoke("get_connection_status") is confirmed working without capabilities.
    if (handlers.onConnection) {
      const poll = async () => {
        try {
          const status = await invoke<ConnectionStatus>("get_connection_status");
          ref.current.onConnection?.(status);
        } catch {
          // Backend not ready or shutting down — ignore
        }
      };
      poll(); // immediate sync on mount
      intervals.push(setInterval(poll, 1000));
    }

    // ── State snapshot ───────────────────────────────────────────────────────
    // Polled every 300 ms — drives PullClock, StatWidgets, Timeline.
    // Replaces the push-based coach:state event (requires capabilities).
    if (handlers.onStateSnapshot) {
      const poll = async () => {
        try {
          const snap = await invoke<StateSnapshot>("get_state_snapshot");
          ref.current.onStateSnapshot?.(snap);
        } catch {
          // Backend not ready or shutting down — ignore
        }
      };
      poll(); // immediate sync on mount
      intervals.push(setInterval(poll, 300));
    }

    // ── Advice queue drain ───────────────────────────────────────────────────
    // Polled every 500 ms — atomically drains all pending advice events from
    // the 50-item ring buffer maintained by ipc::run.
    // Replaces the push-based coach:advice event (requires capabilities).
    if (handlers.onAdvice) {
      const poll = async () => {
        try {
          const events = await invoke<AdviceEvent[]>("drain_advice_queue");
          events.forEach(e => ref.current.onAdvice?.(e));
        } catch {
          // Backend not ready or shutting down — ignore
        }
      };
      poll(); // immediate sync on mount
      intervals.push(setInterval(poll, 500));
    }

    // ── Event log drain ──────────────────────────────────────────────────────
    // Polled every 500 ms — drains the event log ring buffer (connection changes,
    // combat transitions, encounter start/end, advice events).
    // Displayed in the Event Feed sub-tab of the Live Feed panel.
    if (handlers.onEventLog) {
      const poll = async () => {
        try {
          const entries = await invoke<string[]>("drain_event_log");
          if (entries.length > 0) ref.current.onEventLog?.(entries);
        } catch {
          // Backend not ready or shutting down — ignore
        }
      };
      poll(); // immediate sync on mount
      intervals.push(setInterval(poll, 500));
    }

    // Identity and debrief events are not yet polled (deferred to v1.0.5).
    // The engine infers player GUID from player_focus + SPELL_CAST_SUCCESS
    // without a frontend identity event — advice fires as long as player_focus
    // is configured in Settings → "Coached Character".

    return () => {
      intervals.forEach(clearInterval);
    };
  }, []); // Empty deps: set up once on mount, tear down on unmount
}
