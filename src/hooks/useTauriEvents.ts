// Subscribes to Tauri backend events and manages listener cleanup.
//
// Handlers are stored in a ref so the effect doesn't re-subscribe on every
// render when the parent component re-renders — subscriptions are set up
// exactly once and torn down on unmount.
//
// Resilience notes (v1.0.1):
//
// In Tauri v2, `listen()` internally calls `invoke("plugin:event|listen")`,
// which is a plugin-namespaced IPC call.  Without a capabilities/default.json
// that grants `core:default`, this call may silently never resolve in
// production builds, hanging the entire setup() async function and preventing
// `invoke("get_connection_status")` from ever being called.
//
// Two safety measures are added here:
//   1. safeListenWithTimeout — races each listen() against a 3-second timeout.
//      On hang or throw, logs the error to coach.log via log_frontend_error and
//      returns a no-op unlisten so setup() always completes.
//   2. setInterval connection-status polling — calls invoke("get_connection_status")
//      every 1 s regardless of whether listen() resolved.  The invoke() pathway
//      is independently confirmed to work, so the LOG pill always stays correct.
import { useEffect, useRef } from "react";
import { listen, type UnlistenFn, type EventCallback } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  type AdviceEvent,
  type StateSnapshot,
  type ConnectionStatus,
  type PlayerIdentity,
  type PullDebrief,
  EVENT_ADVICE,
  EVENT_STATE,
  EVENT_CONNECTION,
  EVENT_IDENTITY,
  EVENT_DEBRIEF,
} from "../types/events";

// ---------------------------------------------------------------------------
// safeListenWithTimeout
//
// Wraps listen() with:
//   • A .catch() so a thrown error doesn't propagate up through setup()
//   • A Promise.race() timeout so a hanging listen() doesn't block setup()
//
// On error or timeout: logs to console + coach.log (via log_frontend_error),
// then resolves with a no-op unlisten so the rest of setup() continues.
// ---------------------------------------------------------------------------
async function safeListenWithTimeout<T>(
  event: string,
  handler: EventCallback<T>,
  timeoutMs = 3000
): Promise<UnlistenFn> {
  return Promise.race([
    listen<T>(event, handler).catch((err: unknown) => {
      const msg = `listen(${event}) threw: ${String(err)}`;
      console.error("[useTauriEvents]", msg);
      void invoke("log_frontend_error", { msg }).catch(() => {});
      return (): void => {};
    }),
    new Promise<UnlistenFn>((resolve) =>
      setTimeout(() => {
        const msg = `listen(${event}) timed out after ${timeoutMs}ms`;
        console.warn("[useTauriEvents]", msg);
        void invoke("log_frontend_error", { msg }).catch(() => {});
        resolve((): void => {});
      }, timeoutMs)
    ),
  ]);
}

export interface TauriEventHandlers {
  onAdvice?:        (event: AdviceEvent)       => void;
  onStateSnapshot?: (snapshot: StateSnapshot)  => void;
  onConnection?:    (status: ConnectionStatus) => void;
  onIdentity?:      (identity: PlayerIdentity) => void;
  onDebrief?:       (debrief: PullDebrief)     => void;
}

export function useTauriEvents(handlers: TauriEventHandlers): void {
  // Keep latest handlers in a ref so the effect closure never goes stale
  const ref = useRef(handlers);
  ref.current = handlers;

  useEffect(() => {
    const unlisten: UnlistenFn[] = [];
    let connIntervalId: ReturnType<typeof setInterval> | null = null;

    const setup = async () => {
      if (ref.current.onAdvice) {
        unlisten.push(
          await safeListenWithTimeout<AdviceEvent>(EVENT_ADVICE, (e) =>
            ref.current.onAdvice?.(e.payload)
          )
        );
      }
      if (ref.current.onStateSnapshot) {
        unlisten.push(
          await safeListenWithTimeout<StateSnapshot>(EVENT_STATE, (e) =>
            ref.current.onStateSnapshot?.(e.payload)
          )
        );
      }
      if (ref.current.onConnection) {
        unlisten.push(
          await safeListenWithTimeout<ConnectionStatus>(EVENT_CONNECTION, (e) =>
            ref.current.onConnection?.(e.payload)
          )
        );
      }
      if (ref.current.onIdentity) {
        unlisten.push(
          await safeListenWithTimeout<PlayerIdentity>(EVENT_IDENTITY, (e) =>
            ref.current.onIdentity?.(e.payload)
          )
        );
      }
      if (ref.current.onDebrief) {
        unlisten.push(
          await safeListenWithTimeout<PullDebrief>(EVENT_DEBRIEF, (e) =>
            ref.current.onDebrief?.(e.payload)
          )
        );
      }

      // All listen() round-trips complete (or timed out) — sync connection
      // status from managed state so the LOG pill is correct even if the
      // one-shot startup emission fired before listeners were registered.
      if (ref.current.onConnection) {
        try {
          const status = await invoke<ConnectionStatus>("get_connection_status");
          ref.current.onConnection(status);
        } catch {
          // Backend not ready yet — the interval below will pick it up shortly
        }
      }
    };

    setup();

    // ---------------------------------------------------------------------------
    // Connection-status polling (belt-and-suspenders)
    //
    // Polls invoke("get_connection_status") every 1 s regardless of whether
    // listen() resolved.  invoke() is confirmed working even when plugin IPC is
    // broken, so this guarantees the LOG pill stays accurate at all times.
    //
    // Uses handlers.onConnection (initial value, not ref.current) to decide
    // whether to set up the interval, and ref.current inside the callback so
    // the most recent handler is always invoked.
    // ---------------------------------------------------------------------------
    if (handlers.onConnection) {
      const pollConn = async () => {
        try {
          const status = await invoke<ConnectionStatus>("get_connection_status");
          ref.current.onConnection?.(status);
        } catch {
          // Backend not ready or shutting down — ignore
        }
      };
      pollConn(); // immediate sync on mount
      connIntervalId = setInterval(pollConn, 1000);
    }

    return () => {
      if (connIntervalId !== null) clearInterval(connIntervalId);
      unlisten.forEach((fn) => fn());
    };
  }, []); // Empty deps: subscribe once
}
