import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import {
  snapshotTransactionQueries,
  rollbackTransactionQueries,
  patchTransactionInCaches,
} from './optimisticTransactionPatch';

/**
 * Detach a tag from a transaction.
 *
 * Mirrors useAddTransactionTag: resolves the name to an id, patches optimistically,
 * rolls back on failure. Note that removing the last usage of a tag does not
 * delete the tag itself, so the global tag list is not invalidated here.
 *
 * A name matching no known tag resolves to a no-op rather than an error --
 * the desired end state (tag absent) already holds.
 */
export function useRemoveTransactionTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ transactionId, tagName }: { transactionId: string; tagName: string }) => {
      const existingTags = await queryClient.fetchQuery({
        queryKey: queryKeys.tags.list(),
        queryFn: API.tags.list,
      });
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
