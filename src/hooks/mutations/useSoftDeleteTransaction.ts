import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import {
  snapshotTransactionQueries,
  rollbackTransactionQueries,
  removeTransactionFromCaches,
} from './optimisticTransactionPatch';

/**
 * Soft-delete a transaction, removing it from view immediately.
 *
 * Soft rather than hard: the backend flags the row so it can be recovered and
 * so re-ingesting the same source does not resurrect it.
 *
 * Uses onSettled rather than onSuccess -- the caches are refreshed whether the
 * delete succeeded or failed, since a rollback also needs to reconcile against
 * server truth. The dashboard is invalidated too, because its totals include
 * the removed transaction.
 */
export function useSoftDeleteTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (transactionId: string) => API.transactions.delete(transactionId),
    onMutate: async (transactionId) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.transactions.all() });
      const snapshot = snapshotTransactionQueries(queryClient);
      removeTransactionFromCaches(queryClient, transactionId);
      return { snapshot };
    },
    onError: (_err, _vars, context) => {
      if (context) rollbackTransactionQueries(queryClient, context.snapshot);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
      queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
    },
  });
}
