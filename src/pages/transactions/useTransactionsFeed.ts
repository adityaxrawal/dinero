/**
 * Assembles the transaction feed: pagination, filters, and search.
 *
 * Switches between the paginated list and search results depending on whether a
 * query is active, so the feed has one interface regardless of source.
 */
import { useEffect, useMemo, useRef } from 'react';
import type { TransactionListFilters } from '@/lib/ipc';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useTransactionSearch } from '@/hooks/queries/useTransactionSearch';
import { groupByDateLabel } from './groupByDate';

/**
 * Assembles the transaction feed: pagination, filters and search.
 *
 * Switches between the paginated list and search results depending on whether a
 * query is active, so the feed presents one interface regardless of source.
 */
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
