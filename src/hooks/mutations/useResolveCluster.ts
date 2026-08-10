import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

interface ResolveClusterInput {
  clusterId: string;
  observationId: string;
  action: 'confirm_match' | 'reject_candidate' | 'keep_separate' | 'mark_unresolved';
  chosenCanonicalId?: string | undefined;
}

/**
 * Record the user's decision about a reconciliation cluster.
 *
 * One mutation covers all four verdicts -- confirming a match, rejecting a
 * candidate, keeping entries separate, or deferring -- because the backend
 * exposes them as a single resolve command with an action discriminator.
 *
 * The invalidation is broad on purpose: resolving a cluster can merge or split
 * canonical transactions, which changes the ledger and every dashboard figure
 * derived from it, not just the reconciliation queue.
 */
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
