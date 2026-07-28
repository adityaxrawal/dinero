import { useState, useEffect } from 'react';
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';

import { useTransactionDetail } from '@/hooks/queries/useTransactionDetail';
import { useTransactionTags } from '@/hooks/queries/useTransactionTags';
import { useTagsList } from '@/hooks/queries/useTagsList';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';

import { useUpdateTransactionFields } from '@/hooks/mutations/useUpdateTransactionFields';
import { useAddTransactionTag } from '@/hooks/mutations/useAddTransactionTag';
import { useRemoveTransactionTag } from '@/hooks/mutations/useRemoveTransactionTag';
import { useSoftDeleteTransaction } from '@/hooks/mutations/useSoftDeleteTransaction';
import { confirmDeleteTransaction } from '@/lib/confirmDialog';

export function useTransactionForm(transactionId: string | undefined, onClose?: () => void) {
  const { toast } = useToast();

  const { data: detail, isLoading } = useTransactionDetail(transactionId);
  const { data: tags = [] } = useTransactionTags(transactionId);
  const { data: availableTags = [] } = useTagsList();
  const { data: instruments = [] } = useInstrumentsList();
  const { data: categories = [] } = useCategoriesList();

  const updateFields = useUpdateTransactionFields();
  const addTag = useAddTransactionTag();
  const removeTag = useRemoveTransactionTag();
  const softDelete = useSoftDeleteTransaction();

  const [merchant, setMerchant] = useState('');
  const [categoryId, setCategoryId] = useState('');
  const [notes, setNotes] = useState('');
  const [amountStr, setAmountStr] = useState('');
  const [direction, setDirection] = useState<'debit' | 'credit'>('debit');
  const [eventTime, setEventTime] = useState('');
  const [instrumentId, setInstrumentId] = useState('');
  const [newTag, setNewTag] = useState('');
  const [showSavedConfirm, setShowSavedConfirm] = useState(false);

  const tx = detail?.transaction;
  const initialMerchant = tx?.merchant_display_name ?? '';
  const initialCategory = tx?.category_id ?? '';
  const initialNotes = tx?.notes ?? '';
  const initialAmount = tx ? Math.abs(tx.amount ?? (tx.amount_minor ?? 0) / 100).toString() : '0';
  const initialDirection = (tx?.direction === 'credit' ? 'credit' : 'debit') as 'debit' | 'credit';
  const initialEventTime = tx?.best_event_time ?? '';
  const initialInstrumentId = tx?.instrument_id ?? '';

  useEffect(() => {
    if (detail) {
      setMerchant(detail.transaction.merchant_display_name ?? '');
      setCategoryId(detail.transaction.category_id ?? '');
      setNotes(detail.transaction.notes ?? '');
      setAmountStr(Math.abs(detail.transaction.amount ?? (detail.transaction.amount_minor ?? 0) / 100).toString());
      setDirection((detail.transaction.direction === 'credit' ? 'credit' : 'debit'));
      setEventTime(detail.transaction.best_event_time ?? '');
      setInstrumentId(detail.transaction.instrument_id ?? '');
      setShowSavedConfirm(false);
    }
  }, [detail]);

  const handleSave = () => {
    if (!transactionId) return;
    const parsedAmount = parseFloat(amountStr);
    const amountMinor = !isNaN(parsedAmount) ? Math.round(parsedAmount * 100) : undefined;

    updateFields.mutate(
      {
        transactionId,
        merchantDisplayName: merchant,
        categoryId,
        notes,
        amountMinor,
        direction,
        eventTime: eventTime || undefined,
        instrumentId: instrumentId || undefined,
      },
      {
        onSuccess: () => {
          setShowSavedConfirm(true);
          setTimeout(() => setShowSavedConfirm(false), 3000);
        },
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
  };

  const handleAddTag = () => {
    const t = newTag.trim();
    if (!t || tags.includes(t) || !transactionId) return;
    addTag.mutate(
      { transactionId, tagName: t },
      {
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
    setNewTag('');
  };

  const handleRemoveTag = (tag: string) => {
    if (!transactionId) return;
    removeTag.mutate(
      { transactionId, tagName: tag },
      {
        onError: (err) => toast({ variant: 'destructive', ...getErrorToast(err) }),
      }
    );
  };

  const handleDelete = async () => {
    if (!transactionId) return;
    const confirmed = await confirmDeleteTransaction();
    if (!confirmed) return;
    softDelete.mutate(transactionId, {
      onSuccess: () => {
        toast({ title: 'Transaction deleted' });
        if (onClose) onClose();
      },
      onError: (err) =>
        toast({
          variant: 'destructive',
          ...getErrorToast(err, 'Only manually-entered transactions can be deleted.'),
        }),
    });
  };

  const isDirty = Boolean(
    merchant !== initialMerchant ||
    categoryId !== initialCategory ||
    notes !== initialNotes ||
    amountStr !== initialAmount ||
    direction !== initialDirection ||
    eventTime !== initialEventTime ||
    instrumentId !== initialInstrumentId
  );

  const resetForm = () => {
    setMerchant(initialMerchant);
    setCategoryId(initialCategory);
    setNotes(initialNotes);
    setAmountStr(initialAmount);
    setDirection(initialDirection);
    setEventTime(initialEventTime);
    setInstrumentId(initialInstrumentId);
  };

  const amount = parseFloat(amountStr) || (tx ? (tx.amount ?? (tx.amount_minor ?? 0) / 100) : 0);
  const hasEmi = !!tx?.emi_group_id;
  const isDebit = direction === 'debit';
  const selectedInstrument = instrumentId
    ? instruments.find((i) => i.id === instrumentId)
    : undefined;
  const category = categories.find((c) => c.id === categoryId);
  const isForeignCurrency = Boolean(
    !!tx?.original_amount_minor && tx.original_currency && tx.original_currency !== tx.currency
  );

  return {
    detail,
    isLoading,
    tags,
    availableTags,
    instruments,
    categories,
    merchant,
    setMerchant,
    categoryId,
    setCategoryId,
    notes,
    setNotes,
    amountStr,
    setAmountStr,
    direction,
    setDirection,
    eventTime,
    setEventTime,
    instrumentId,
    setInstrumentId,
    newTag,
    setNewTag,
    showSavedConfirm,
    isDirty,
    resetForm,
    updateFields,
    softDelete,
    tx,
    amount,
    hasEmi,
    isDebit,
    instrument: selectedInstrument,
    category,
    isForeignCurrency,
    handleSave,
    handleAddTag,
    handleRemoveTag,
    handleDelete,
  };
}
