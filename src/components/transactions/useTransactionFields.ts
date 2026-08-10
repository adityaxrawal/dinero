/**
 * Field-level state and validation for transaction editing.
 */
import { useState, useEffect } from 'react';
import type { CanonicalTransaction } from '@/lib/ipc';
import { fieldsFromTransaction, type TransactionFields } from './transactionFormFields';

const EMPTY: TransactionFields = {
  merchant: '',
  categoryId: '',
  notes: '',
  amountStr: '',
  direction: 'debit',
  eventTime: '',
  instrumentId: '',
};

/** Field-level state and validation for transaction editing. */
export function useTransactionFields(tx: CanonicalTransaction | undefined, loaded: boolean) {
  const [fields, setFields] = useState<TransactionFields>(EMPTY);
  const [showSavedConfirm, setShowSavedConfirm] = useState(false);

  const initial = fieldsFromTransaction(tx);

  useEffect(() => {
    if (!loaded || !tx) return;
    setFields(fieldsFromTransaction(tx));
    setShowSavedConfirm(false);
  }, [tx, loaded]);

  const setField =
    <K extends keyof TransactionFields>(name: K) =>
    (value: TransactionFields[K]) =>
      setFields((prev) => ({ ...prev, [name]: value }));

  const isDirty = (Object.keys(initial) as (keyof TransactionFields)[]).some(
    (key) => fields[key] !== initial[key]
  );

  return {
    fields,
    setField,
    initial,
    isDirty,
    resetForm: () => setFields(initial),
    showSavedConfirm,
    setShowSavedConfirm,
  };
}
