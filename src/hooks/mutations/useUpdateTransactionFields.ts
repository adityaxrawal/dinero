import { useMutation, useQueryClient } from '@tanstack/react-query';
import { API } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';

interface UpdateTransactionFieldsInput {
  transactionId: string;
  merchantDisplayName?: string | undefined;
  categoryId?: string | undefined;
  notes?: string | undefined;
  amountMinor?: number | undefined;
  direction?: string | undefined;
  eventTime?: string | undefined;
  instrumentId?: string | undefined;
}

/**
 * Edit user-changeable fields on a transaction.
 *
 * Every field is optional and only those supplied are sent, so a caller editing
 * one value cannot inadvertently blank the others. Deliberately not optimistic:
 * edits here can change amount, direction or date, which shift derived figures
 * across the app, and showing an unconfirmed value would be misleading.
 */
export function useUpdateTransactionFields() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      transactionId,
      merchantDisplayName,
      categoryId,
      notes,
      amountMinor,
      direction,
      eventTime,
      instrumentId,
    }: UpdateTransactionFieldsInput) =>
      API.transactions.update(transactionId, {
        merchantDisplayName,
        categoryId,
        notes,
        amountMinor,
        direction,
        eventTime,
        instrumentId,
      }),
    onSuccess: (_data, { transactionId }) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.detail(transactionId) });
      queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
    },
  });
}
