/**
 * Form state for the transaction inspector, including dirty tracking.
 */
import { useState } from 'react';
import { useTransactionDetail } from '@/hooks/queries/useTransactionDetail';
import { useTransactionTags } from '@/hooks/queries/useTransactionTags';
import { useTagsList } from '@/hooks/queries/useTagsList';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useCategoriesList } from '@/hooks/queries/useCategoriesList';
import { useTransactionFields } from './useTransactionFields';
import { useTransactionMutations } from './useTransactionMutations';
import { isForeignCurrencyTransaction, transactionAmount } from './transactionFormFields';

/** Form state for the transaction inspector, with dirty tracking. */
export function useTransactionForm(transactionId: string | undefined, onClose?: () => void) {
  const { data: detail, isLoading } = useTransactionDetail(transactionId);
  const { data: tags = [] } = useTransactionTags(transactionId);
  const { data: availableTags = [] } = useTagsList();
  const { data: instruments = [] } = useInstrumentsList();
  const { data: categories = [] } = useCategoriesList();

  const tx = detail?.transaction;
  const [newTag, setNewTag] = useState('');
  const form = useTransactionFields(tx, !!detail);
  const { fields, setField } = form;

  const mutations = useTransactionMutations({
    transactionId,
    fields,
    tags,
    newTag,
    setNewTag,
    setShowSavedConfirm: form.setShowSavedConfirm,
    onClose,
  });

  return {
    detail,
    isLoading,
    tags,
    availableTags,
    instruments,
    categories,

    merchant: fields.merchant,
    setMerchant: setField('merchant'),
    categoryId: fields.categoryId,
    setCategoryId: setField('categoryId'),
    notes: fields.notes,
    setNotes: setField('notes'),
    amountStr: fields.amountStr,
    setAmountStr: setField('amountStr'),
    direction: fields.direction,
    setDirection: setField('direction'),
    eventTime: fields.eventTime,
    setEventTime: setField('eventTime'),
    instrumentId: fields.instrumentId,
    setInstrumentId: setField('instrumentId'),
    newTag,
    setNewTag,

    showSavedConfirm: form.showSavedConfirm,
    isDirty: form.isDirty,
    resetForm: form.resetForm,

    tx,
    amount: parseFloat(fields.amountStr) || (tx ? transactionAmount(tx) : 0),
    hasEmi: !!tx?.emi_group_id,
    isDebit: fields.direction === 'debit',
    instrument: instruments.find((i) => i.id === fields.instrumentId),
    category: categories.find((c) => c.id === fields.categoryId),
    isForeignCurrency: isForeignCurrencyTransaction(tx),

    ...mutations,
  };
}
