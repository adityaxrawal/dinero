import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Issue #7: re-runs the statement pipeline over the whole Action Needed
 * queue using stored passwords.
 */
export function useReparseAllStatements() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => API.statements.reparseAll(),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.statements.all() });
    },
  });
}
