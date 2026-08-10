import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Fetches the fixed spending-category taxonomy.
 *
 * Effectively static reference data, so it is cached under a single key and
 * shared by every category picker in the app rather than re-fetched per screen.
 */
export function useCategoriesList() {
  return useQuery({
    queryKey: queryKeys.categories.list(),
    queryFn: API.categories.list,
  });
}
