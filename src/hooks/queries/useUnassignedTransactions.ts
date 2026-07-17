import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/** TASK-FE-013: see useReconciliationClusters for the refetchOnMount rationale. */
export function useUnassignedTransactions() {
  return useQuery({
    queryKey: queryKeys.reconciliation.unassigned(),
    queryFn: API.reconciliation.listUnassigned,
    refetchOnMount: 'always',
  });
}
