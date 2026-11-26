import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * How many emails are waiting for the local AI, and whether it can run
 * (`ready`, `no_model`, `ineligible`). Refreshed by the
 * `unassigned_changed` event (every queued job and every AI verdict emits it),
 * through the reconciliation key family.
 */
export function useLlmQueueStatus() {
  return useQuery({
    queryKey: queryKeys.reconciliation.llmPending(),
    queryFn: API.reconciliation.llmQueueStatus,
    refetchOnMount: 'always',
  });
}
