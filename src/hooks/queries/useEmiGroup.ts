import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/** TASK-FE-010: only enabled when a transaction actually has an `emi_group_id`. */
export function useEmiGroup(emiGroupId: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.transactions.emiGroup(emiGroupId ?? ''),
    queryFn: () => API.transactions.getEmiGroup(emiGroupId as string),
    enabled: !!emiGroupId,
  });
}
