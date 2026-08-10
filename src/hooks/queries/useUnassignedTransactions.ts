import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Transactions that could not be attributed to any known instrument.
 *
 * Always refetched on mount for the same reason as the cluster list: it is a
 * work queue, and stale entries would send the user to items already resolved.
 */
export function useUnassignedTransactions() {
  return useQuery({
    queryKey: queryKeys.reconciliation.unassigned(),
    queryFn: API.reconciliation.listUnassigned,
    refetchOnMount: 'always',
  });
}
