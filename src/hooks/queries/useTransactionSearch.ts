import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import type { TransactionListFilters } from '@/lib/ipc';

/**
 * Full-text transaction search, scoped by the current filters.
 *
 * Stays disabled for an empty or whitespace-only query, so clearing the search
 * box does not fire a request that would match the entire ledger. Callers are
 * expected to debounce the query before passing it in.
 */
export function useTransactionSearch(query: string, filters?: TransactionListFilters) {
  return useQuery({
    queryKey: queryKeys.transactions.search(query, filters),
    queryFn: () => API.transactions.search(query, filters),
    enabled: query.trim().length > 0,
  });
}

