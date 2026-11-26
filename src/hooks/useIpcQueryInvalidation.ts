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
  // A new alert can create the card it was paid with (audit N-B1).
  {
    event: 'transaction_created',
    keys: [queryKeys.transactions.all(), queryKeys.dashboard.all(), queryKeys.instruments.all()],
  },
  // Instruments too: an edit can move a transaction to another instrument, and
  // per-instrument totals would stay stale (audit F12-3).
  {
    event: 'transaction_updated',
    keys: [queryKeys.transactions.all(), queryKeys.dashboard.all(), queryKeys.instruments.all()],
  },
  {
    event: 'transaction_deleted',
    keys: [queryKeys.transactions.all(), queryKeys.dashboard.all(), queryKeys.instruments.all()],
  },
  {
    event: 'scan_completed',
    keys: [
      queryKeys.transactions.all(),
      queryKeys.dashboard.all(),
      queryKeys.statements.all(),
      queryKeys.instruments.all(),
      // A scan files review entries; the queue and its badge must catch up.
      queryKeys.reconciliation.all(),
      queryKeys.activity.all(),
    ],
  },
  // A failed or cancelled scan is recorded in the activity log too.
  { event: 'scan_failed', keys: [queryKeys.activity.all()] },
  { event: 'scan_cancelled', keys: [queryKeys.activity.all()] },
  {
    event: 'reconciliation_cluster',
    keys: [queryKeys.reconciliation.all(), queryKeys.dashboard.all()],
  },
  // A review entry was filed, relabelled or settled in the background (B-1).
  // Dashboard too: its "Pending Review" tile counts these entries (audit F14-1).
  {
    event: 'unassigned_changed',
    keys: [queryKeys.reconciliation.all(), queryKeys.dashboard.all()],
  },
  // Statement pipeline events refresh the Statements screen's history and
  // queue, which otherwise only caught up after a scan finished (audit F8-4).
  ...[
    'statement_parsed',
    'statement_parse_failed',
    'statement_duplicate_rejected',
    'statement_password_required',
    'statement_staged',
    'statement_instrument_confirmation_required',
  ].map((event) => ({
    event,
    // A committed statement also adds an activity entry.
    keys: event === 'statement_parsed' ? [queryKeys.statements.all(), queryKeys.activity.all()] : [queryKeys.statements.all()],
  })),
  {
    event: 'statement_upcoming_bill_set',
    keys: [queryKeys.dashboard.all(), queryKeys.instruments.all()],
  },
];

/** How long invalidations are collected before refetching (audit B-2). */
export const INVALIDATION_BATCH_MS = 500;

/**
 * Collects query keys and invalidates each once per batch window. A scan emits
 * one `transaction_created` per job; invalidating on every event refetched the
 * dashboard aggregates continuously (audit B-2).
 */
export function createInvalidationBatcher(
  invalidate: (key: QueryKey) => void,
  delayMs = INVALIDATION_BATCH_MS
): { add: (keys: readonly QueryKey[]) => void; cancel: () => void } {
  const pending = new Map<string, QueryKey>();
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    timer = null;
    const keys = [...pending.values()];
    pending.clear();
    keys.forEach(invalidate);
  };
  return {
    add(keys) {
      for (const key of keys) pending.set(JSON.stringify(key), key);
      if (timer === null) timer = setTimeout(flush, delayMs);
    },
    cancel() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      pending.clear();
    },
  };
}

/** Invalidates the matching caches when the backend reports a change. */
export function useIpcQueryInvalidation(): void {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    const batcher = createInvalidationBatcher((queryKey) =>
      queryClient.invalidateQueries({ queryKey })
    );

    /** Subscribes to every event in the invalidation table. */
    const subscribe = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      for (const { event, keys } of EVENT_INVALIDATIONS) {
        const unlisten = await listen(event, () => batcher.add(keys));
        // Unsubscribing can reject if the listener is already gone, which is
        // harmless during teardown. Wrapped so that one failure cannot abort
        // the cleanup loop and strand the remaining subscriptions.
        const safeUnlisten = () => {
          Promise.resolve(unlisten()).catch((e) => {
            console.debug(
              'Failed to unlisten to IPC event (likely already unlistened or component unmounted):',
              e
            );
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
      batcher.cancel();
      unlisteners.forEach((fn) => fn());
    };
  }, [queryClient]);
}
