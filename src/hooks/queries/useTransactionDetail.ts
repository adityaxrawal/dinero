import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * A single transaction with its observations and match decisions.
 *
 * Returns far more than the list view: the source observations are what let the
 * detail screen explain where each field came from and how confident the
 * extraction was. Disabled while the id is absent.
 */
export function useTransactionDetail(id: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.transactions.detail(id ?? ''),
    queryFn: () => API.transactions.get(id as string),
    enabled: !!id,
  });
}
