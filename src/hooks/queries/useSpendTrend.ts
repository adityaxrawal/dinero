import { useQuery } from '@tanstack/react-query';
import { API, SpendTrendGranularity } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useSpendTrend(granularity: SpendTrendGranularity) {
  return useQuery({
    queryKey: queryKeys.dashboard.spendTrend(granularity),
    queryFn: () => API.analytics.getSpendTrend(granularity),
  });
}
