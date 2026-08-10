import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Permanently dismiss a statement that failed to parse.
 *
 * Distinct from discarding a draft: nothing was ever extracted here, and the
 * user is declining to retry, so the entry leaves the retry panel for good.
 */
export function useDiscardUnprocessedStatement() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (statementId: string) => API.statements.discard(statementId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
