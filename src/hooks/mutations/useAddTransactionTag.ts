import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import {
  snapshotTransactionQueries,
  rollbackTransactionQueries,
  patchTransactionInCaches,
} from './optimisticTransactionPatch';

/**
 * Attach a tag to a transaction, creating the tag if it does not yet exist.
 *
 * Tags are addressed by name in the UI but by id in the backend, so this
 * resolves one to the other: an existing tag is reused, and only a genuinely new
 * name causes a create. Matching is case-insensitive, which prevents "Travel"
 * and "travel" from becoming two separate tags.
 *
 * The update is optimistic -- the tag appears immediately and is rolled back if
 * the backend rejects it.
 */
export function useAddTransactionTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ transactionId, tagName }: { transactionId: string; tagName: string }) => {
      const trimmed = tagName.trim();
      // fetchQuery rather than a plain read: the cached tag list may be stale,
      // and acting on a stale list would create a duplicate tag.
      const existingTags = await queryClient.fetchQuery({
        queryKey: queryKeys.tags.list(),
        queryFn: API.tags.list,
      });
      const existing = existingTags.find((t) => t.name.toLowerCase() === trimmed.toLowerCase());
      const tagId = existing ? existing.id : (await API.tags.create(trimmed)).id;
      await API.transactions.addTag(transactionId, tagId);
      return tagId;
    },
    onMutate: async ({ transactionId, tagName }) => {
      // In-flight refetches are cancelled first; one landing mid-update would
      // overwrite the optimistic patch with pre-mutation server data.
      await queryClient.cancelQueries({ queryKey: queryKeys.transactions.all() });
      const snapshot = snapshotTransactionQueries(queryClient);

      const trimmed = tagName.trim();
      patchTransactionInCaches(queryClient, transactionId, (tx) => {
        const currentTags = tx.tags ?? [];
        // Already present: return the record unchanged so the reference is
        // stable and no needless re-render is triggered.
        if (currentTags.some((t) => t.toLowerCase() === trimmed.toLowerCase())) {
          return tx;
        }
        return {
          ...tx,
          tags: [...currentTags, trimmed],
        };
      });

      return { snapshot };
    },
    onError: (_err, _vars, context) => {
      if (context) rollbackTransactionQueries(queryClient, context.snapshot);
    },
    // Three separate invalidations, because a tag addition touches three
    // things: this transaction's tags, the global tag list (which may have
    // gained a new entry), and the transaction lists whose rows display tags.
    onSuccess: (_data, { transactionId }) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.tags(transactionId) });
      queryClient.invalidateQueries({ queryKey: queryKeys.tags.list() });
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
    },
  });
}
