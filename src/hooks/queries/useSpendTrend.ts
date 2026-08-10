import { useQuery } from '@tanstack/react-query';
import { API, SpendTrendGranularity } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Spend over time at the requested granularity, for the trend chart.
 *
 * Granularity is part of the key, so switching between daily, weekly and
 * monthly views caches each series independently.
 */
export function useSpendTrend(granularity: SpendTrendGranularity) {
  return useQuery({
    queryKey: queryKeys.dashboard.spendTrend(granularity),
    queryFn: () => API.analytics.getSpendTrend(granularity),
  });
}
