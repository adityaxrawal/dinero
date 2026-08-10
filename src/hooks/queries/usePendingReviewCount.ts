import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * How many transactions are awaiting user review, and their total value.
 *
 * Drives the review badge, so it must reflect reconciliation activity promptly.
 */
export function usePendingReviewCount() {
  return useQuery({
    queryKey: queryKeys.dashboard.pendingReview(),
    queryFn: API.analytics.getPendingReviewCount,
  });
}
