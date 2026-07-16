import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import {
  snapshotTransactionQueries,
  rollbackTransactionQueries,
  patchTransactionInCaches,
} from './optimisticTransactionPatch';

/** TASK-FE-009: inline category-change quick action, applied optimistically. */
export function useUpdateTransactionCategory() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ transactionId, categoryId }: { transactionId: string; categoryId: string }) =>
      API.transactions.update(transactionId, { categoryId }),
    onMutate: async ({ transactionId, categoryId }) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.transactions.all() });
      const snapshot = snapshotTransactionQueries(queryClient);
      patchTransactionInCaches(queryClient, transactionId, (tx) => ({ ...tx, category: categoryId }));
      return { snapshot };
    },
    onError: (_err, _vars, context) => {
      if (context) rollbackTransactionQueries(queryClient, context.snapshot);
    },
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
    },
  });
}
