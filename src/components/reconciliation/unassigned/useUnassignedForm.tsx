/**
 * Form state for manually resolving an unassigned transaction.
 */
import { useState, useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API, type UnassignedTransactionRecord } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { useResolveUnassignedTransaction } from '@/hooks/mutations/useResolveUnassignedTransaction';
import { ToastAction } from '@/components/ui/toast';
import { engineDecisionToast } from '@/lib/engineOutcome';

/** Form state for manually resolving an unassigned transaction. */
export function useUnassignedForm(
  record: UnassignedTransactionRecord | undefined,
  onClose: () => void
) {
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const resolveManually = useResolveUnassignedTransaction();

  const [merchant, setMerchant] = useState('');
  const [amount, setAmount] = useState('');
  const [direction, setDirection] = useState<'debit' | 'credit'>('debit');
  const [date, setDate] = useState('');
  const [instrumentId, setInstrumentId] = useState('');
  const [referenceId, setReferenceId] = useState('');

  useEffect(() => {
    setMerchant(record?.merchant_raw ?? '');
    setAmount(record?.amount_minor != null ? (record.amount_minor / 100).toString() : '');
    setDirection(record?.direction === 'credit' ? 'credit' : 'debit');
    setDate(record?.event_time ? record.event_time.slice(0, 10) : '');
    setInstrumentId('');
    setReferenceId('');
  }, [
    record?.id,
    record?.merchant_raw,
    record?.amount_minor,
    record?.direction,
    record?.event_time,
  ]);

  const canSubmit = Boolean(merchant.trim() && amount && date && instrumentId);

  /** Dismisses the entry without resolving it. */
  /** Refreshes everything a review entry's status can change. */
  const invalidateReview = () => {
    queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.all() });
    queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
  };

  // Dismiss is one click, so it must be undoable and must never fail silently
  // (audit F9-4).
  const handleDismiss = async () => {
    if (!record) return;
    const id = record.id;
    try {
      await API.reconciliation.dismissUnassigned(id);
    } catch (err) {
      toast({ variant: 'destructive', ...getErrorToast(err) });
      return;
    }
    invalidateReview();
    onClose();
    toast({
      title: 'Marked as not a transaction',
      description: 'Removed from the review queue.',
      action: (
        <ToastAction
          altText="Undo"
          onClick={() => {
            API.reconciliation
              .restoreUnassigned(id)
              .then(invalidateReview)
              .catch((e) => toast({ variant: 'destructive', ...getErrorToast(e) }));
          }}
        >
          Undo
        </ToastAction>
      ),
    });
  };

  const handleSave = () => {
    if (!record || !canSubmit) return;
    resolveManually.mutate(
      {
        id: record.id,
        amountMinor: Math.round(parseFloat(amount) * 100),
        currency: record.currency ?? 'INR',
        direction,
        eventTime: `${date} 00:00:00`,
        merchantName: merchant.trim(),
        instrumentId,
        referenceId: referenceId.trim() || undefined,
      },
      {
        onSuccess: (decision) => {
          toast(engineDecisionToast(decision));
          onClose();
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
  };

  /** Applies a value spotted in the source email to a form field. */
  const applyQuickFill = ({ field, value }: { field: string; value: string }) => {
    const setters: Record<string, (v: string) => void> = {
      amount: setAmount,
      merchant: setMerchant,
      date: setDate,
      referenceId: setReferenceId,
    };
    setters[field]?.(value);
    toast({ title: 'Quick-Fill Applied', description: `Updated ${field} to "${value}"` });
  };

  return {
    fields: { merchant, amount, direction, date, instrumentId, referenceId },
    setters: { setMerchant, setAmount, setDirection, setDate, setInstrumentId, setReferenceId },
    canSubmit,
    isPending: resolveManually.isPending,
    handleDismiss,
    handleSave,
    applyQuickFill,
  };
}
