import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Installment schedule for a transaction that is part of an EMI plan.
 *
 * Disabled until an id exists -- most transactions are not EMI purchases, so the
 * hook is mounted unconditionally and simply stays idle for the rest.
 */
export function useEmiGroup(emiGroupId: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.transactions.emiGroup(emiGroupId ?? ''),
    queryFn: () => API.transactions.getEmiGroup(emiGroupId as string),
    enabled: !!emiGroupId,
  });
}
