import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API, DraftMetadataInput, DraftRow } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Commit a reviewed statement draft, turning its rows into real transactions.
 *
 * The terminal step of statement import: the user has confirmed the extracted
 * metadata and rows, and this writes them into the ledger. Invalidating the
 * whole statements tree afterwards refreshes both the history list and the
 * awaiting-review queue the draft just left.
 */
export function useCommitStatementDraft() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      draftId,
      metadata,
      rows,
    }: {
      draftId: string;
      metadata: DraftMetadataInput;
      rows: DraftRow[];
    }) => API.statements.commitDraft(draftId, metadata, rows),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
