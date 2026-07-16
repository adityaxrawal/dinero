import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useTransactionDetail(id: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.transactions.detail(id ?? ''),
    queryFn: () => API.transactions.get(id as string),
    enabled: !!id,
  });
}
