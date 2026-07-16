import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/** TASK-FE-009: FTS5-backed search (query is expected to already be debounced by the caller). */
export function useTransactionSearch(query: string) {
  return useQuery({
    queryKey: queryKeys.transactions.search(query),
    queryFn: () => API.transactions.search(query),
    enabled: query.trim().length > 0,
  });
}
