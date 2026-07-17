import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

/** TASK-FE-011: InstrumentDetail's "forget saved password" action. */
export function useForgetPdfPassword() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (id: string) => API.pdfPasswords.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.pdfPasswords.all() });
    },
  });
}
