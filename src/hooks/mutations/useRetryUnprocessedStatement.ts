import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useRetryUnprocessedStatement() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (statementId: string) => API.statements.retryUnprocessed(statementId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
