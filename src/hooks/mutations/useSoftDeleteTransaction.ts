import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import {
  snapshotTransactionQueries,
  rollbackTransactionQueries,
  removeTransactionFromCaches,
} from './optimisticTransactionPatch';

/**
 * TASK-FE-009: inline soft-delete quick action (with confirmation, handled
 * by the calling component before this fires). Backend restricts deletion
 * to manually-entered transactions — a rejection just rolls the optimistic
 * removal back and surfaces the real error.
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
