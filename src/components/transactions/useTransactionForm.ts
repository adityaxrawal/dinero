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
  const [newTag, setNewTag] = useState('');
  const [showSavedConfirm, setShowSavedConfirm] = useState(false);

  useEffect(() => {
    if (detail) {
      setMerchant(detail.transaction.merchant_display_name ?? '');
      setCategoryId(detail.transaction.category_id ?? '');
      setNotes(detail.transaction.notes ?? '');
      setShowSavedConfirm(false);
    }
  }, [detail]);

  const handleSave = () => {
    if (!transactionId) return;
    updateFields.mutate(
      { transactionId, merchantDisplayName: merchant, categoryId, notes },
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

  const tx = detail?.transaction;
  const amount = tx ? (tx.amount ?? (tx.amount_minor ?? 0) / 100) : 0;
  const hasEmi = !!tx?.emi_group_id;
  const isDebit = tx?.direction === 'debit';
  const instrument = tx?.instrument_id
    ? instruments.find((i) => i.id === tx.instrument_id)
    : undefined;
  const category = categories.find((c) => c.id === tx?.category_id);
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
    newTag,
    setNewTag,
    showSavedConfirm,
    updateFields,
    softDelete,
    tx,
    amount,
    hasEmi,
    isDebit,
    instrument,
    category,
    isForeignCurrency,
    handleSave,
    handleAddTag,
    handleRemoveTag,
    handleDelete,
  };
}
