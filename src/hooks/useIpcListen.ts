import { useEffect, useRef } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/**
 * Subscribe to a Tauri backend event for the lifetime of a component.
 *
 * Solves two problems that make raw `listen` awkward inside React. The handler
 * is held in a ref and kept current, so the subscription is established once per
 * event name rather than being torn down whenever an inline callback's identity
 * changes -- while still always invoking the latest handler. And because
 * `listen` resolves asynchronously, the `cancelled` flag releases a subscription
 * that arrives after unmount, which would otherwise leak.
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
