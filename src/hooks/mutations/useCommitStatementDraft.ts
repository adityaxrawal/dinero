import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API, DraftMetadataInput, DraftRow } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

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
