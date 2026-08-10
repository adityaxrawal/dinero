import { useInfiniteQuery } from '@tanstack/react-query';
import { API, TransactionListFilters } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

// Must match the backend's page size. If the two disagree, the short-page
// termination check below misfires and paging either stops early or never ends.
const PAGE_SIZE = 50;

/**
 * Infinite-scrolling transaction list for the main ledger view.
 *
 * The whole filter object forms part of the query key, so changing any filter
 * starts a fresh pagination sequence rather than appending mismatched pages to
 * the previous one.
 */
export function useTransactionsInfiniteList(filters: TransactionListFilters) {
  return useInfiniteQuery({
    queryKey: queryKeys.transactions.infiniteList(filters),
    queryFn: ({ pageParam }) => API.transactions.list(pageParam, filters),
    initialPageParam: 1,
    // Two independent stop conditions, either of which ends pagination by
    // returning undefined. The running total guards against requesting past the
    // reported end, and the short-page check catches the case where the backend
    // total is stale or approximate -- a page smaller than PAGE_SIZE can only be
    // the last one.
    getNextPageParam: (lastPage, allPages) => {
      const loadedSoFar = allPages.reduce((sum, p) => sum + p.records.length, 0);
      if (loadedSoFar >= lastPage.total || lastPage.records.length < PAGE_SIZE) return undefined;
      return allPages.length + 1;
    },
  });
}
