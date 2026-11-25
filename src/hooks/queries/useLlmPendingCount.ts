import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * How many emails the local AI is still reading. Refreshed by the
 * `unassigned_changed` event (every queued job and every AI verdict emits it),
 * through the reconciliation key family.
 */
export function useLlmPendingCount() {
  return useQuery({
    queryKey: queryKeys.reconciliation.llmPending(),
    queryFn: API.reconciliation.llmPendingCount,
    refetchOnMount: 'always',
  });
}
