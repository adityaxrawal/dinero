import { useEffect } from 'react';
import { useQueryClient, type QueryKey } from '@tanstack/react-query';
import { queryKeys } from '@/lib/queryKeys';
import { isTauriRuntime } from '@/lib/tauriRuntime';

/**
 * Keeps the React Query cache honest by reacting to backend mutations.
 *
 * The Rust side changes data on its own schedule -- a background scan writes
 * transactions, reconciliation merges records -- with no frontend mutation
 * involved. Without this bridge the UI would keep serving cached data until
 * something happened to refetch it.
 *
 * The mapping is declarative rather than imperative: one table below states
 * which caches each event invalidates, so adding a new event means adding a row
 * rather than writing another subscription.
 */

/**
 * Backend event to affected cache keys.
 *
 * Note that most events invalidate more than their obvious target, because the
 * dashboard aggregates the ledger -- any transaction change also invalidates
 * every figure derived from it. `scan_completed` is the broadest, since a scan
 * can produce transactions, statements and instruments in one pass.
 *
 * Exported for tests, which assert this table against the events the backend
 * actually emits.
 */
export const EVENT_INVALIDATIONS: ReadonlyArray<{ event: string; keys: readonly QueryKey[] }> = [
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
  {
    event: 'reconciliation_cluster',
    keys: [queryKeys.reconciliation.all(), queryKeys.dashboard.all()],
  },
  {
    event: 'statement_upcoming_bill_set',
    keys: [queryKeys.dashboard.all(), queryKeys.instruments.all()],
  },
];

/** Invalidates the matching caches when the backend reports a change. */
export function useIpcQueryInvalidation(): void {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    /** Subscribes to every event in the invalidation table. */
    const subscribe = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      for (const { event, keys } of EVENT_INVALIDATIONS) {
        const unlisten = await listen(event, () => {
          for (const key of keys) {
            queryClient.invalidateQueries({ queryKey: key });
          }
        });
        // Unsubscribing can reject if the listener is already gone, which is
        // harmless during teardown. Wrapped so that one failure cannot abort
        // the cleanup loop and strand the remaining subscriptions.
        const safeUnlisten = () => {
          Promise.resolve(unlisten()).catch((e) => {
            console.debug('Failed to unlisten to IPC event (likely already unlistened or component unmounted):', e);
          });
        };

        // Same post-unmount race as elsewhere: subscriptions are established
        // asynchronously, so any that resolve after cleanup are released at once.
        if (cancelled) {
          safeUnlisten();
        } else {
          unlisteners.push(safeUnlisten);
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
