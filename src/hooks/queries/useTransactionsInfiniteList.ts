import { useInfiniteQuery } from '@tanstack/react-query';
import { API, TransactionListFilters } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

// Matches src-tauri/src/commands/data.rs's TRANSACTIONS_PAGE_SIZE — the
// server always returns this many rows per page regardless of what the
// caller asks for, so this is only used to detect the last page (a short
// page means no more data), not to request a page size.
const PAGE_SIZE = 50;

/**
 * TASK-FE-009 (Doc 30): infinite-scroll pagination via React Query's
 * useInfiniteQuery. `filters` is included in the query key so switching any
 * filter starts a fresh paginated list rather than mixing pages fetched
 * under different filter combinations.
 */
export function useTransactionsInfiniteList(filters: TransactionListFilters) {
  return useInfiniteQuery({
    queryKey: queryKeys.transactions.infiniteList(filters),
    queryFn: ({ pageParam }) => API.transactions.list(pageParam, filters),
    initialPageParam: 1,
    getNextPageParam: (lastPage, allPages) => {
      const loadedSoFar = allPages.reduce((sum, p) => sum + p.records.length, 0);
      if (loadedSoFar >= lastPage.total || lastPage.records.length < PAGE_SIZE) return undefined;
      return allPages.length + 1;
    },
  });
}
