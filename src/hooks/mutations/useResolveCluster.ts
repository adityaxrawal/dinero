import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

interface ResolveClusterInput {
  clusterId: string;
  observationId: string;
  action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved';
  chosenCanonicalId?: string | undefined;
}

export function useResolveCluster() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: ResolveClusterInput) =>
      API.reconciliation.resolve(
        input.clusterId,
        input.observationId,
        input.action,
        input.chosenCanonicalId
      ),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.all() });
      queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
    },
  });
}
