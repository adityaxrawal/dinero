import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * One reconciliation cluster with its members, for the comparison view.
 *
 * Disabled until the route parameter resolves to a cluster id.
 */
export function useReconciliationCluster(clusterId: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.reconciliation.cluster(clusterId ?? ''),
    queryFn: () => API.reconciliation.getCluster(clusterId as string),
    enabled: !!clusterId,
  });
}
