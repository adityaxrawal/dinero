import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useReconciliationCluster(clusterId: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.reconciliation.cluster(clusterId ?? ''),
    queryFn: () => API.reconciliation.getCluster(clusterId as string),
    enabled: !!clusterId,
  });
}
