import { useEffect, useRef } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/**
 * TASK-SETUP-013. Subscribes to a Tauri event for the lifetime of the
 * calling component.
 *
 * Several existing components (`AppLayout.tsx`, `GlobalStateContext.tsx`,
 * `Transactions.tsx`, `SpendingLimits.tsx`) hand-roll this exact
 * `useEffect` + `listen()` + `unlisten()` pattern already — this hook
 * consolidates it for new code going forward. Existing call sites are not
 * migrated here (out of this task's scope).
 */
export function useIpcListen<T>(event: string, handler: (payload: T) => void): void {
  const handlerRef = useRef(handler);
  useEffect(() => {
    handlerRef.current = handler;
  }, [handler]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    listen<T>(event, (e) => handlerRef.current(e.payload)).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [event]);
}
