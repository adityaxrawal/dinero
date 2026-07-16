import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useTransactionsList(page = 1) {
  return useQuery({
    queryKey: queryKeys.transactions.list(page),
    queryFn: () => API.transactions.list(page),
  });
}
