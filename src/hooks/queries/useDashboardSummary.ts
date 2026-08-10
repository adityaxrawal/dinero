import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Headline dashboard figures: month-to-date spend, limit, utilisation, income.
 *
 * Backs the summary tiles at the top of the dashboard. Invalidated whenever a
 * transaction changes, since every tile derives from the ledger.
 */
export function useDashboardSummary() {
  return useQuery({
    queryKey: queryKeys.dashboard.summary(),
    queryFn: API.dashboard.getSummary,
  });
}
