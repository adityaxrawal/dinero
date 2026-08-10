import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Every payment instrument the user has registered.
 *
 * Read across the app -- instrument pickers, attribution dialogs, the
 * instruments screen -- all sharing one cache entry.
 */
export function useInstrumentsList() {
  return useQuery({
    queryKey: queryKeys.instruments.list(),
    queryFn: API.instruments.list,
  });
}
