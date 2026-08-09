import { useEffect, useMemo, useRef } from 'react';
import type { TransactionListFilters } from '@/lib/ipc';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useTransactionSearch } from '@/hooks/queries/useTransactionSearch';
import { groupByDateLabel } from './groupByDate';

/** The feed itself: paged list, or search results while a query is active. */
export function useTransactionsFeed(
  filters: TransactionListFilters,
  searchQuery: string,
  isSearching: boolean
) {
  const infinite = useTransactionsInfiniteList(filters);
  const search = useTransactionSearch(searchQuery, filters);

  const listed = useMemo(
    () => infinite.data?.pages.flatMap((p) => p.records) ?? [],
    [infinite.data]
  );
  const transactions = useMemo(
    () => (isSearching ? (search.data ?? []) : listed),
    [isSearching, search.data, listed]
  );

  const { hasNextPage, isFetchingNextPage, fetchNextPage } = infinite;

  // Infinite scroll sentinel
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (isSearching || !hasNextPage) return;
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting && !isFetchingNextPage) fetchNextPage();
      },
      { rootMargin: '200px' }
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [isSearching, hasNextPage, isFetchingNextPage, fetchNextPage]);

  const grouped = useMemo(() => groupByDateLabel(transactions), [transactions]);

  return {
    transactions,
    grouped,
    loading: isSearching ? search.isLoading : infinite.isLoading,
    total: isSearching ? transactions.length : (infinite.data?.pages[0]?.total ?? 0),
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
    sentinelRef,
  };
}
