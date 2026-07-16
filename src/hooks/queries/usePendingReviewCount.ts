import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function usePendingReviewCount() {
  return useQuery({
    queryKey: queryKeys.dashboard.pendingReview(),
    queryFn: API.analytics.getPendingReviewCount,
  });
}
