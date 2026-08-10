import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Bills predicted to fall due soon, for the dashboard's upcoming widget.
 */
export function useUpcomingBills() {
  return useQuery({
    queryKey: queryKeys.dashboard.upcomingBills(),
    queryFn: API.dashboard.getUpcomingBills,
  });
}
