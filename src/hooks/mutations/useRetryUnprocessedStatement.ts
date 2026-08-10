import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Re-attempt parsing of a statement that previously failed.
 *
 * Worth retrying because failures are often circumstantial -- a password since
 * supplied, or an instrument created after the first attempt.
 */
export function useRetryUnprocessedStatement() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (statementId: string) => API.statements.retryUnprocessed(statementId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
