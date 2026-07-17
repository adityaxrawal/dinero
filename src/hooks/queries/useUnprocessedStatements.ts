import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-FE-012: TASK-STMT-010's real 3-bucket unprocessed-items backend.
 * `refetchOnMount: 'always'` — this queue directly drives user
 * action-or-discard decisions, so it should never show a stale snapshot
 * from a previous visit to this page just because it's within the global
 * 30s staleTime window.
 */
export function useUnprocessedStatements() {
  return useQuery({
    queryKey: queryKeys.statements.unprocessed(),
    queryFn: API.statements.listUnprocessed,
    refetchOnMount: 'always',
  });
}
