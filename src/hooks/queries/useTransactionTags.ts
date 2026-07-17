import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useTransactionTags(transactionId: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.transactions.tags(transactionId ?? ''),
    queryFn: () => API.transactions.getTags(transactionId as string),
    enabled: !!transactionId,
  });
}
