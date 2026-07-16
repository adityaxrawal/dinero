import { useEffect } from 'react';
import { useQueryClient, type QueryKey } from '@tanstack/react-query';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-FE-003 (Doc 30): subscribes to backend push events and invalidates
 * the matching React Query cache entries, so background ingestion (Gmail
 * poll, historical scan, reconciliation) shows up without the user manually
 * refreshing and without the frontend polling. Mount once near the app root
 * (`IpcEventBridge` in `App.tsx`).
 *
 * Doc30's illustrative event names (`transaction.created`, `sync.completed`)
 * are paraphrases — the real event strings are the snake_case ones in
 * `src-tauri/src/ipc/events.rs`'s `AppEvent::as_str()` (established
 * precedent from TASK-FE-002's `useSyncStore`/`useLicenseStore`).
 */
const EVENT_INVALIDATIONS: ReadonlyArray<{ event: string; keys: readonly QueryKey[] }> = [
  { event: 'transaction_created', keys: [queryKeys.transactions.all(), queryKeys.dashboard.all()] },
  { event: 'transaction_updated', keys: [queryKeys.transactions.all(), queryKeys.dashboard.all()] },
  { event: 'transaction_deleted', keys: [queryKeys.transactions.all(), queryKeys.dashboard.all()] },
  {
    event: 'scan_completed',
    keys: [
      queryKeys.transactions.all(),
      queryKeys.dashboard.all(),
      queryKeys.statements.all(),
      queryKeys.instruments.all(),
    ],
  },
  { event: 'reconciliation_cluster', keys: [queryKeys.reconciliation.all(), queryKeys.dashboard.all()] },
  { event: 'statement_upcoming_bill_set', keys: [queryKeys.dashboard.all(), queryKeys.instruments.all()] },
];

export function useIpcQueryInvalidation(): void {
  const queryClient = useQueryClient();

  useEffect(() => {
    const isTauriRuntime =
      typeof window !== 'undefined' &&
      !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    if (!isTauriRuntime) return;

    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    const subscribe = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      for (const { event, keys } of EVENT_INVALIDATIONS) {
        const unlisten = await listen(event, () => {
          for (const key of keys) {
            queryClient.invalidateQueries({ queryKey: key });
          }
        });
        if (cancelled) {
          unlisten();
        } else {
          unlisteners.push(unlisten);
        }
      }
    };

    subscribe().catch((e) => console.error('Failed to subscribe to IPC invalidation events', e));

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [queryClient]);
}
