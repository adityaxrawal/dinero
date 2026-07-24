import type { InfiniteData, QueryClient } from '@tanstack/react-query';
import type { TransactionsPage, TransactionRecord } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-FE-009: shared optimistic-update plumbing for the inline quick
 * actions (category change / tag add / soft-delete) — patches every cached
 * `transactions.*` infinite-list page in place, returning a snapshot for
 * rollback on error. All three mutations use this rather than duplicating
 * the cancel/snapshot/patch/rollback dance three times.
 */
export function snapshotTransactionQueries(queryClient: QueryClient) {
  return queryClient.getQueriesData<InfiniteData<TransactionsPage>>({
    queryKey: queryKeys.transactions.all(),
  });
}

export function rollbackTransactionQueries(
  queryClient: QueryClient,
  snapshot: ReturnType<typeof snapshotTransactionQueries>
) {
  snapshot.forEach(([key, data]) => queryClient.setQueryData(key, data));
}

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

export function removeTransactionFromCaches(queryClient: QueryClient, transactionId: string) {
  _updateTransactionCaches(queryClient, (page) => ({
    ...page,
    records: page.records.filter((r) => r.id !== transactionId),
    total: Math.max(0, page.total - 1),
  }));
}
