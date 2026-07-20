import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import {
  snapshotTransactionQueries,
  rollbackTransactionQueries,
  patchTransactionInCaches,
} from './optimisticTransactionPatch';

/**
 * TASK-FE-010: resolves the tag name to its real id via `tags_list`, then
 * calls the dedicated `transactions_remove_tag` command (Doc19 §8.8) —
 * see `useAddTransactionTag` for why this no longer goes through
 * `transactions_update`'s bulk-replace workaround.
 */
export function useRemoveTransactionTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ transactionId, tagName }: { transactionId: string; tagName: string }) => {
      const existingTags = await queryClient.fetchQuery({ queryKey: queryKeys.tags.list(), queryFn: API.tags.list });
      const tag = existingTags.find((t) => t.name.toLowerCase() === tagName.toLowerCase());
      if (!tag) return;
      await API.transactions.removeTag(transactionId, tag.id);
    },
    onMutate: async ({ transactionId, tagName }) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.transactions.all() });
      const snapshot = snapshotTransactionQueries(queryClient);
      
      patchTransactionInCaches(queryClient, transactionId, (tx) => ({
        ...tx,
        tags: (tx.tags ?? []).filter((t) => t.toLowerCase() !== tagName.toLowerCase()),
      }));
      
      return { snapshot };
    },
    onError: (_err, _vars, context) => {
      if (context) rollbackTransactionQueries(queryClient, context.snapshot);
    },
    onSuccess: (_data, { transactionId }) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.tags(transactionId) });
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
    },
  });
}
