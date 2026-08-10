import type { InfiniteData, QueryClient } from '@tanstack/react-query';
import type { TransactionsPage, TransactionRecord } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Cache surgery helpers behind the optimistic transaction mutations.
 *
 * Transactions are cached in several places at once -- the paginated list, the
 * infinite list, and every filtered variant of both -- so an optimistic update
 * cannot simply patch one entry. These helpers operate across every cache under
 * the `transactions` key prefix, keeping all views consistent while a mutation
 * is in flight.
 *
 * The pattern each mutation follows is snapshot, patch, and roll back on error.
 * Snapshotting first is what makes the optimism safe: a rejected mutation
 * restores exactly what was there before, with no re-fetch required.
 *
 * All updates are immutable, since React Query compares by reference to decide
 * what re-renders.
 */

/**
 * Capture the current contents of every transaction query, for rollback.
 *
 * Returns key/data pairs rather than a merged object, so each cache entry can
 * be restored to precisely its own previous value.
 */
export function snapshotTransactionQueries(queryClient: QueryClient) {
  return queryClient.getQueriesData<InfiniteData<TransactionsPage>>({
    queryKey: queryKeys.transactions.all(),
  });
}

/** Restore every captured cache entry, undoing an optimistic update. */
export function rollbackTransactionQueries(
  queryClient: QueryClient,
  snapshot: ReturnType<typeof snapshotTransactionQueries>
) {
  snapshot.forEach(([key, data]) => queryClient.setQueryData(key, data));
}

/**
 * Apply a page transform across every transaction cache.
 *
 * The shared engine behind both public mutators below. Each cache holds
 * infinite-query data, so the transform is mapped over every loaded page; an
 * absent cache is returned untouched rather than being created.
 */
function _updateTransactionCaches(
  queryClient: QueryClient,
  updatePage: (page: TransactionsPage) => TransactionsPage
) {
  queryClient.setQueriesData<InfiniteData<TransactionsPage>>(
    { queryKey: queryKeys.transactions.all() },
    (old) => {
      if (!old) return old;
      return {
        ...old,
        pages: old.pages.map(updatePage),
      };
    }
  );
}

/**
 * Rewrite one transaction wherever it appears, leaving the rest untouched.
 *
 * The patch callback receives the current record and returns its replacement,
 * which lets callers express edits in terms of the existing value.
 */
export function patchTransactionInCaches(
  queryClient: QueryClient,
  transactionId: string,
  patch: (tx: TransactionRecord) => TransactionRecord
) {
  _updateTransactionCaches(queryClient, (page) => ({
    ...page,
    records: page.records.map((r) => (r.id === transactionId ? patch(r) : r)),
  }));
}

/**
 * Drop a transaction from every cache, for optimistic deletion.
 *
 * The page total is decremented alongside the removal so paging maths and any
 * "N transactions" label stay consistent with the rows actually present. It is
 * floored at zero because the same page can be visited by more than one cache
 * entry, and a double decrement would otherwise produce a negative count.
 */
export function removeTransactionFromCaches(queryClient: QueryClient, transactionId: string) {
  _updateTransactionCaches(queryClient, (page) => ({
    ...page,
    records: page.records.filter((r) => r.id !== transactionId),
    total: Math.max(0, page.total - 1),
  }));
}
