/**
 * Form state for the instrument inspector, including dirty tracking.
 */
import { useState, useEffect, useCallback } from 'react';
import type { InstrumentRecord } from '@/lib/ipc';
import { useInstrumentDetail } from '@/hooks/queries/useInstrumentDetail';
import { useForgetPdfPassword } from '@/hooks/mutations/useForgetPdfPassword';
import { type InstrumentFormFields } from './instrumentUpdate';
import { useInstrumentActions } from './useInstrumentActions';
import { useInstrumentRelated } from './useInstrumentRelated';

const EMPTY_FIELDS: InstrumentFormFields = {
  issuerName: '',
  maskedIdentifier: '',
  nickname: '',
  fullIdentifier: '',
  billingCycleDay: '',
  bankIfsc: '',
  instrumentType: 'credit_card',
  status: 'active',
  creditLimit: '',
  network: '',
  accountType: '',
  upiVpa: '',
  rewardsSummary: '',
  statementDueDate: '',
  minimumDue: '',
};

/** Projects an instrument into editable form fields. */
function fieldsFromInstrument(inst: InstrumentRecord): InstrumentFormFields {
  return {
    issuerName: inst.issuer_name ?? '',
    maskedIdentifier: inst.masked_identifier ?? '',
    nickname: inst.nickname ?? '',
    fullIdentifier: inst.full_identifier ?? '',
    billingCycleDay: inst.billing_cycle_day?.toString() ?? '',
    bankIfsc: inst.bank_ifsc ?? '',
    instrumentType: inst.instrument_type ?? 'credit_card',
    status: inst.status ?? 'active',
    creditLimit: inst.credit_limit?.toString() ?? '',
    network: inst.network ?? '',
    accountType: inst.account_type ?? '',
    upiVpa: inst.upi_vpa ?? '',
    rewardsSummary: inst.rewards_summary ?? '',
    statementDueDate: inst.statement_due_date ?? '',
    minimumDue: inst.minimum_due != null ? (inst.minimum_due / 100.0).toString() : '',
  };
}

/** Form state for the inspector, with dirty tracking. */
export function useInstrumentForm(
  instrumentId: string | undefined,
  initialInstrument?: InstrumentRecord,
  onClose?: () => void
) {

  const { data: detailInst, isLoading } = useInstrumentDetail(instrumentId);
  const forgetPassword = useForgetPdfPassword();

  const [fields, setFields] = useState<InstrumentFormFields>(EMPTY_FIELDS);
  const setField = useCallback(
    <K extends keyof InstrumentFormFields>(name: K, value: InstrumentFormFields[K]) =>
      setFields((prev) => ({ ...prev, [name]: value })),
    []
  );

  const inst = detailInst ?? initialInstrument;
  const isNegative = (inst?.current_balance ?? 0) < 0;

  const { isSaving, isDeleting, showSavedConfirm, setShowSavedConfirm, handleSave, handleDelete } =
    useInstrumentActions(inst, fields, onClose);
  const related = useInstrumentRelated(instrumentId, inst);

  useEffect(() => {
    const active = detailInst ?? initialInstrument;
    if (active) {
      setFields(fieldsFromInstrument(active));
      setShowSavedConfirm(false);
    }
  }, [detailInst, initialInstrument, setShowSavedConfirm]);

  return {
    inst,
    isLoading,
    detailInst,
    forgetPassword,
    fields,
    setField,
    isSaving,
    isDeleting,
    showSavedConfirm,
    isNegative,
    handleSave,
    handleDelete,
    ...related,
  };
}
