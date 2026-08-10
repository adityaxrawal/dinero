import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';

/**
 * Delete a saved statement-PDF password from the OS keychain.
 *
 * Toasts on both outcomes, since this is a settings action with no other visual
 * confirmation that anything happened.
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
