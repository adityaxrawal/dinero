import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useInstrumentDetail(id: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.instruments.detail(id ?? ''),
    queryFn: () => API.instruments.get(id as string),
    enabled: !!id,
  });
}
