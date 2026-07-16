import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useInstrumentsList() {
  return useQuery({
    queryKey: queryKeys.instruments.list(),
    queryFn: API.instruments.list,
  });
}
