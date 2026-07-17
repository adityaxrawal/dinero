import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-FE-013: `refetchOnMount: 'always'` — this queue drives real
 * financial-record-altering decisions, so revisiting it should never show
 * a stale snapshot from a previous visit just because it's within the
 * global 30s staleTime window (same rationale as `useUnprocessedStatements`,
 * TASK-FE-012).
 */
export function useReconciliationClusters() {
  return useQuery({
    queryKey: queryKeys.reconciliation.unresolved(),
    queryFn: API.reconciliation.listUnresolved,
    refetchOnMount: 'always',
  });
}
