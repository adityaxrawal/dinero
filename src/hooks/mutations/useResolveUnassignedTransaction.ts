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
  referenceId?: string | undefined;
}

/**
 * Manually attribute an unassigned transaction to an instrument.
 *
 * The fallback path for transactions extraction could not attribute
 * automatically. The user supplies the full set of fields, so this both
 * completes the record and removes it from the unassigned queue.
 */
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
