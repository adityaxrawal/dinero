import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useDiscardStatementDraft() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (draftId: string) => API.statements.discardDraft(draftId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
