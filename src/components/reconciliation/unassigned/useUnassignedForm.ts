import { useState, useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API, type UnassignedTransactionRecord } from '@/lib/ipc';
import { queryKeys } from '@/lib/queryKeys';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { useResolveUnassignedTransaction } from '@/hooks/mutations/useResolveUnassignedTransaction';

/** The editable copy of an unassigned record, reset whenever the record changes. */
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

  const handleDismiss = async () => {
    if (!record) return;
    try {
      await API.reconciliation.dismissUnassigned(record.id);
      queryClient.invalidateQueries({ queryKey: queryKeys.reconciliation.unassigned() });
      onClose();
    } catch (err) {
      console.error('Failed to dismiss', err);
    }
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
        onSuccess: () => {
          toast({ title: 'Transaction saved' });
          onClose();
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
  };

  /** Applied from the email evidence pane's Quick-Fill buttons. */
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
