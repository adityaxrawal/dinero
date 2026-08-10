import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * History of every imported statement and its processing outcome.
 *
 * Note the statements screen also reloads this through the global statement
 * event layer, which reacts to backend parse events as they arrive.
 */
export function useStatementsList() {
  return useQuery({
    queryKey: queryKeys.statements.list(),
    queryFn: () => API.statements.listHistory(),
  });
}
