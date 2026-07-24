import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

interface ResolveUnassignedInput {
  id: string;
  amountMinor: number;
  currency: string;
  direction: 'credit' | 'debit';
  eventTime: string;
  merchantName: string;
  instrumentId: string;
  referenceId?: string;
}

export function useResolveUnassignedTransaction() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (input: ResolveUnassignedInput) =>
      API.reconciliation.resolveUnassignedManually(input.id, input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.unassigned() });
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
      queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
    },
  });
}
