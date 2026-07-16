import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-FE-009: inline tag-add quick action.
 *
 * Uses `transactions_update`'s bulk `tags: string[]` replace convenience
 * (tag *names*), not the dedicated `transactions_add_tag` command — that
 * command requires a real tag UUID (`validate_uuid("tag_id", ...)`), but
 * `tags_list` (the only tag catalog exposed to the frontend) explicitly
 * discards ids and returns names only (`tags.into_iter().map(|t| t.name)`
 * in commands/mod.rs). There is currently no frontend-reachable way to
 * resolve a tag name to its id, so `transactions_add_tag`/`_remove_tag` are
 * unusable from a name-based UI — flagged in fix-log, not fixed here (fixing
 * it means changing `tags_list`'s shape, which is Area 8/API-007 scope).
 * The bulk-replace path is what the pre-existing detail drawer already used
 * successfully, so this follows proven precedent rather than the broken one.
 *
 * Fetches current tags first (bulk-replace, not append) — genuinely two
 * round trips, not optimistic in the truest sense, but correct.
 */
export function useAddTransactionTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ transactionId, tagName }: { transactionId: string; tagName: string }) => {
      const currentTags = await API.transactions.getTags(transactionId);
      if (currentTags.includes(tagName)) return currentTags;
      const nextTags = [...currentTags, tagName];
      await API.transactions.update(transactionId, { tags: nextTags });
      return nextTags;
    },
    onSuccess: (_data, { transactionId }) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.tags(transactionId) });
    },
  });
}
