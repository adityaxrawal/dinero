import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';

/**
 * TASK-FE-011: InstrumentDetail's "forget saved password" action.
 * Success/error toasts live here rather than in each caller -- InstrumentDetail
 * and InstrumentInspector both used to hand-roll the identical pair.
 */
export function useForgetPdfPassword() {
  const queryClient = useQueryClient();
  const { toast } = useToast();

  return useMutation({
    mutationFn: (id: string) => API.pdfPasswords.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.pdfPasswords.all() });
      toast({ title: 'Saved password forgotten' });
    },
    onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
  });
}
