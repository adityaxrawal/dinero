import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useDiscardUnprocessedStatement() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (statementId: string) => API.statements.discard(statementId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
