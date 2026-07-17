import { useQuery } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

export function useStatementsList() {
  return useQuery({
    queryKey: queryKeys.statements.list(),
    queryFn: () => API.statements.listHistory(),
  });
}
