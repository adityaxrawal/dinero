import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useUpcomingBills() {
  return useQuery({
    queryKey: queryKeys.dashboard.upcomingBills(),
    queryFn: API.dashboard.getUpcomingBills,
  });
}
