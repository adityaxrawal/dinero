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
 * TASK-FE-010: the detail page's "Save Corrections" action — Document 19
 * §8.3's editable set (merchant_display_name, category_id, notes), distinct
 * from the list page's narrower single-field quick-action mutations
 * (TASK-FE-009). Not optimistic (this is a deliberate full-form submit, not
 * an inline quick toggle) — invalidates on success instead.
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
