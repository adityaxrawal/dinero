import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import type { TransactionListFilters } from '@/lib/ipc';

/** TASK-FE-009: FTS5-backed search (query is expected to already be debounced by the caller). */
export function useTransactionSearch(query: string, filters?: TransactionListFilters) {
  return useQuery({
    queryKey: queryKeys.transactions.search(query, filters),
    queryFn: () => API.transactions.search(query, filters),
    enabled: query.trim().length > 0,
  });
}

