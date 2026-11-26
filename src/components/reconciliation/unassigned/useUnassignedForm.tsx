/**
 * Form state for manually resolving an unassigned transaction.
 */
import { useState, useEffect, useRef } from 'react';
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
  // '' until chosen: when the email gave no direction the user must pick one,
  // instead of a silent 'debit' (owner decision Q-9).
  const [direction, setDirection] = useState<'debit' | 'credit' | ''>('');
  const [date, setDate] = useState('');
  // Optional HH:MM. Blank means "date only" (audit F9-6: changing the date
  // used to force midnight, with no way to enter the real time).
  const [time, setTime] = useState('');
  const [instrumentId, setInstrumentId] = useState('');
  const [referenceId, setReferenceId] = useState('');

  // What was last loaded from the record, to tell the user's edits apart from
  // a background update (audit F9-10: the local AI finishing an entry used to
  // overwrite whatever the user had typed).
  const loaded = useRef<{ id: string | undefined; values: string[] } | null>(null);
  const typed = [merchant, amount, direction, date, time, instrumentId, referenceId];
  const [updatedInBackground, setUpdatedInBackground] = useState(false);

  const loadFromRecord = () => {
    const printed = record?.event_time?.slice(11, 16) ?? '';
    const values = [
      record?.merchant_raw ?? '',
      record?.amount_minor != null ? (record.amount_minor / 100).toString() : '',
      record?.direction === 'credit' || record?.direction === 'debit' ? record.direction : '',
      record?.event_time ? record.event_time.slice(0, 10) : '',
      printed && printed !== '00:00' ? printed : '',
      '',
      '',
    ];
    setMerchant(values[0]);
    setAmount(values[1]);
    setDirection(values[2] as 'debit' | 'credit' | '');
    setDate(values[3]);
    setTime(values[4]);
    setInstrumentId('');
    setReferenceId('');
    loaded.current = { id: record?.id, values };
    setUpdatedInBackground(false);
  };

  useEffect(() => {
    const prev = loaded.current;
    const edited =
      !!prev && prev.id === record?.id && typed.some((v, i) => v !== prev.values[i]);
    if (edited) {
      // Same entry, new values, and the user has typed: keep their edits and
      // offer the new values instead of replacing them.
      setUpdatedInBackground(true);
      return;
    }
    loadFromRecord();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    record?.id,
    record?.merchant_raw,
    record?.amount_minor,
    record?.direction,
    record?.event_time,
  ]);

  const canSubmit = Boolean(merchant.trim() && amount && date && instrumentId && direction);

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
    if (!record || !canSubmit || !direction) return;
    resolveManually.mutate(
      {
        id: record.id,
        amountMinor: Math.round(parseFloat(amount) * 100),
        currency: record.currency ?? 'INR',
        direction,
        eventTime: `${date} ${time || '00:00'}:00`,
        merchantName: merchant.trim(),
        instrumentId,
        referenceId: referenceId.trim() || undefined,
      },
      {
        onSuccess: (decision) => {
          // The email named the bank and card type but no number: offer to
          // remember this pick (owner decision Q-6). Nothing is assumed
          // unless the user confirms.
          const issuer = record.rule_issuer;
          const kind = record.rule_instrument_type;
          const chosen = instrumentId;
          toast({
            ...engineDecisionToast(decision),
            ...(issuer && kind
              ? {
                  action: (
                    <ToastAction
                      altText="Always use this card"
                      onClick={() => {
                        API.instrumentRules
                          .create(issuer, kind, chosen)
                          .then(() =>
                            toast({
                              title: 'Rule saved',
                              description: `Future ${issuer} ${kind.replace('_', ' ')} alerts without a card number will use this card. Change it under Learned Rules.`,
                            })
                          )
                          .catch((e) => toast({ variant: 'destructive', ...getErrorToast(e) }));
                      }}
                    >
                      Always use this card
                    </ToastAction>
                  ),
                }
              : {}),
          });
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
    fields: { merchant, amount, direction, date, time, instrumentId, referenceId },
    updatedInBackground,
    loadLatest: loadFromRecord,
    setters: {
      setMerchant,
      setAmount,
      setDirection,
      setDate,
      setTime,
      setInstrumentId,
      setReferenceId,
    },
    canSubmit,
    isPending: resolveManually.isPending,
    handleDismiss,
    handleSave,
    applyQuickFill,
  };
}
