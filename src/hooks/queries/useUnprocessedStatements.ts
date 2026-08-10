import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Statements that failed to parse and are awaiting a retry or a password.
 *
 * Always refetched on mount so the retry panel reflects the true current state
 * rather than a cached snapshot from before the last attempt.
 */
export function useUnprocessedStatements() {
  return useQuery({
    queryKey: queryKeys.statements.unprocessed(),
    queryFn: API.statements.listUnprocessed,
    refetchOnMount: 'always',
  });
}
