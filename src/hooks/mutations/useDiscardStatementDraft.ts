import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Discard a statement draft without committing any of its rows.
 *
 * The reject path opposite useCommitStatementDraft, used when extraction
 * produced something the user does not want in their ledger.
 */
export function useDiscardStatementDraft() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (draftId: string) => API.statements.discardDraft(draftId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
