import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Per-category spend totals for one month of the dashboard breakdown.
 *
 * The month is part of the query key, so each month is cached separately and
 * flipping back to a previously viewed month is instant.
 */
export function useDashboardCategories(month: string) {
  return useQuery({
    queryKey: queryKeys.dashboard.categories(month),
    queryFn: () => API.dashboard.getCategories(month),
  });
}
