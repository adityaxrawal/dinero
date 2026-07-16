import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useCategoriesList() {
  return useQuery({
    queryKey: queryKeys.categories.list(),
    queryFn: API.categories.list,
  });
}
