import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * One payment instrument (card or account) by id.
 *
 * Disabled while the id is absent, which is the state during route transitions
 * before the parameter has resolved.
 */
export function useInstrumentDetail(id: string | null | undefined) {
  return useQuery({
    queryKey: queryKeys.instruments.detail(id ?? ''),
    queryFn: () => API.instruments.get(id as string),
    enabled: !!id,
  });
}
