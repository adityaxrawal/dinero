import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * One page of transactions, for the paginated ledger view.
 *
 * The page number is part of the key, so each page caches separately.
 * See useTransactionsInfiniteList for the scroll-based variant.
 */
export function useTransactionsList(page = 1) {
  return useQuery({
    queryKey: queryKeys.transactions.list(page),
    queryFn: () => API.transactions.list(page),
  });
}
