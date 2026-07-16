import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/** `month` is a "YYYY-MM" string (Document 19 §11.3's exact argument shape). */
export function useDashboardCategories(month: string) {
  return useQuery({
    queryKey: queryKeys.dashboard.categories(month),
    queryFn: () => API.dashboard.getCategories(month),
  });
}
