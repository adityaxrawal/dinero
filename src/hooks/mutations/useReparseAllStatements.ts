import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Re-run extraction across every stored statement.
 *
 * A bulk maintenance action, used after parser improvements so previously
 * mis-extracted statements can be reprocessed with newer logic. Long-running:
 * progress arrives through the background task events, not this mutation.
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
