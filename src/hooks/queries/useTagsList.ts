import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * All user-defined tags, for pickers and filters.
 *
 * Invalidated when a tag is created or removed so every picker updates at once.
 */
export function useTagsList() {
  return useQuery({
    queryKey: queryKeys.tags.list(),
    queryFn: API.tags.list,
  });
}
