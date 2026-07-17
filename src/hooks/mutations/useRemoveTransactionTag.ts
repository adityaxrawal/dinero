import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-FE-010: same bulk-replace workaround as `useAddTransactionTag`
 * (TASK-FE-009) — `transactions_remove_tag` needs a real tag UUID that
 * `tags_list` never exposes, so this goes through `transactions_update`'s
 * name-based `tags: string[]` replace instead.
 */
export function useRemoveTransactionTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ transactionId, tagName }: { transactionId: string; tagName: string }) => {
      const currentTags = await API.transactions.getTags(transactionId);
      const nextTags = currentTags.filter((t) => t !== tagName);
      await API.transactions.update(transactionId, { tags: nextTags });
      return nextTags;
    },
    onSuccess: (_data, { transactionId }) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.tags(transactionId) });
    },
  });
}
