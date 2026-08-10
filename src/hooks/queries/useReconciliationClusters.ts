import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * All unresolved duplicate clusters awaiting a merge-or-split decision.
 *
 * `refetchOnMount: 'always'` overrides the global staleTime deliberately: this
 * list drives a badge count and a work queue, and showing a cluster the user
 * already resolved -- or missing one that just appeared -- is worse here than
 * the cost of an extra fetch on navigation.
 */
export function useReconciliationClusters() {
  return useQuery({
    queryKey: queryKeys.reconciliation.unresolved(),
    queryFn: API.reconciliation.listUnresolved,
    refetchOnMount: 'always',
  });
}
