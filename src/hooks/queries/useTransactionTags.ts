import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Tags attached to one transaction.
 *
 * Kept as its own query rather than being folded into the transaction detail,
 * so adding or removing a tag can refresh just this slice.
 */
export function useTransactionTags(transactionId: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.transactions.tags(transactionId ?? ''),
    queryFn: () => API.transactions.getTags(transactionId as string),
    enabled: !!transactionId,
  });
}
