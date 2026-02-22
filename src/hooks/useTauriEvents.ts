// Subscribes to Tauri backend events and manages listener cleanup.
//
// Handlers are stored in a ref so the effect doesn't re-subscribe on every
// render when the parent component re-renders — subscriptions are set up
// exactly once and torn down on unmount.
import { useEffect, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

    const setup = async () => {
      if (ref.current.onAdvice) {
        unlisten.push(
          await listen<AdviceEvent>(EVENT_ADVICE, (e) =>
            ref.current.onAdvice?.(e.payload)
          )
        );
      }
      if (ref.current.onStateSnapshot) {
        unlisten.push(
          await listen<StateSnapshot>(EVENT_STATE, (e) =>
            ref.current.onStateSnapshot?.(e.payload)
          )
        );
      }
      if (ref.current.onConnection) {
        unlisten.push(
          await listen<ConnectionStatus>(EVENT_CONNECTION, (e) =>
            ref.current.onConnection?.(e.payload)
          )
        );
      }
      if (ref.current.onIdentity) {
        unlisten.push(
          await listen<PlayerIdentity>(EVENT_IDENTITY, (e) =>
            ref.current.onIdentity?.(e.payload)
          )
        );
      }
      if (ref.current.onDebrief) {
        unlisten.push(
          await listen<PullDebrief>(EVENT_DEBRIEF, (e) =>
            ref.current.onDebrief?.(e.payload)
          )
        );
      }

      // All listen() round-trips complete — sync connection status from managed
      // state so the LOG pill is correct even if the one-shot startup emission
      // fired before this listener was registered (common on app restart).
      if (ref.current.onConnection) {
        try {
          const status = await invoke<ConnectionStatus>("get_connection_status");
          ref.current.onConnection(status);
        } catch {
          // Backend not ready yet — the live event will arrive shortly
        }
      }
    };

    setup();
    return () => { unlisten.forEach((fn) => fn()); };
  }, []); // Empty deps: subscribe once
}
