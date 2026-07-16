import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useTagsList() {
  return useQuery({
    queryKey: queryKeys.tags.list(),
    queryFn: API.tags.list,
  });
}
