import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function usePdfPasswordsList() {
  return useQuery({
    queryKey: queryKeys.pdfPasswords.list(),
    queryFn: API.pdfPasswords.list,
  });
}
