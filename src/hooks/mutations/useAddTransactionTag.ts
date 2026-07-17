import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-FE-009: inline tag-add quick action.
 *
 * Resolves the typed tag name against the real tag catalog (`tags_list`,
 * now id-bearing) and creates it if new, then calls the dedicated
 * `transactions_add_tag` command (Doc19 §8.7) with the real tag id —
 * previously this went through `transactions_update`'s name-based bulk
 * `tags` replace because `tags_list` discarded ids; fixed by exposing ids
 * (see `tags_list` in commands/mod.rs) rather than keeping the workaround.
 */
export function useAddTransactionTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ transactionId, tagName }: { transactionId: string; tagName: string }) => {
      const trimmed = tagName.trim();
      const existingTags = await queryClient.fetchQuery({ queryKey: queryKeys.tags.list(), queryFn: API.tags.list });
      const existing = existingTags.find((t) => t.name.toLowerCase() === trimmed.toLowerCase());
      const tagId = existing ? existing.id : (await API.tags.create(trimmed)).id;
      await API.transactions.addTag(transactionId, tagId);
      return tagId;
    },
    onSuccess: (_data, { transactionId }) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.tags(transactionId) });
      queryClient.invalidateQueries({ queryKey: queryKeys.tags.list() });
    },
  });
}
