import { useState, useEffect, useCallback } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API, InstrumentRecord } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import { useToast } from '@/hooks/use-toast';
import { confirmAction } from '@/lib/confirmDialog';
import { queryKeys } from '@/lib/queryKeys';
import { useInstrumentDetail } from '@/hooks/queries/useInstrumentDetail';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useStatementsList } from '@/hooks/queries/useStatementsList';
import { usePdfPasswordsList } from '@/hooks/queries/usePdfPasswordsList';
import { useForgetPdfPassword } from '@/hooks/mutations/useForgetPdfPassword';

/** The editable half of an instrument, all held as strings while being typed. */
interface InstrumentFormFields {
  issuerName: string;
  maskedIdentifier: string;
  nickname: string;
  fullIdentifier: string;
  billingCycleDay: string;
  bankIfsc: string;
  instrumentType: string;
  status: string;
  creditLimit: string;
  network: string;
  accountType: string;
  upiVpa: string;
  rewardsSummary: string;
  statementDueDate: string;
  minimumDue: string;
}

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
    // Stored in minor units, edited in major.
    minimumDue: inst.minimum_due != null ? (inst.minimum_due / 100.0).toString() : '',
  };
}

export function useInstrumentForm(
  instrumentId: string | undefined,
  initialInstrument?: InstrumentRecord,
  onClose?: () => void
) {
  const { toast } = useToast();
  const queryClient = useQueryClient();

  const { data: detailInst, isLoading } = useInstrumentDetail(instrumentId);
  const {
    data: txData,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading: isTxLoading,
  } = useTransactionsInfiniteList(instrumentId ? { instrument_id: instrumentId } : {});

  const { data: statements = [] } = useStatementsList();
  const { data: pdfPasswords = [] } = usePdfPasswordsList();
  const forgetPassword = useForgetPdfPassword();

  // One record rather than 15 useState pairs: the flat version had to be
  // named twice in the hook's return and a third time in every consumer's
  // destructure, which is what fallow flagged as dup:900f8eb6.
  const [fields, setFields] = useState<InstrumentFormFields>(EMPTY_FIELDS);
  const setField = useCallback(
    <K extends keyof InstrumentFormFields>(name: K, value: InstrumentFormFields[K]) =>
      setFields((prev) => ({ ...prev, [name]: value })),
    []
  );

  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [showSavedConfirm, setShowSavedConfirm] = useState(false);

  useEffect(() => {
    const active = detailInst ?? initialInstrument;
    if (active) {
      setFields(fieldsFromInstrument(active));
      setShowSavedConfirm(false);
    }
  }, [detailInst, initialInstrument]);

  const inst = detailInst ?? initialInstrument;
  const isNegative = (inst?.current_balance ?? 0) < 0;

  const recentTransactions = txData?.pages.flatMap((page) => page.records) ?? [];
  const totalTxCount = txData?.pages[0]?.total ?? recentTransactions.length;
  const instrumentStatements = inst ? statements.filter((s) => s.instrument_id === inst.id) : [];
  const instrumentPasswords = inst ? pdfPasswords.filter((p) => p.instrument_id === inst.id) : [];

  const handleSave = async () => {
    if (!inst) return;
    setIsSaving(true);
    try {
      const extra: {
        nickname?: string;
        credit_limit?: number;
        account_type?: string;
        network?: string;
        status?: string;
        upi_vpa?: string;
        rewards_summary?: string;
        instrument_type?: string;
        issuer_name?: string;
        masked_identifier?: string;
        statement_due_date?: string;
        minimum_due?: number;
      } = {};
      if (fields.nickname) extra.nickname = fields.nickname;
      if (fields.creditLimit) extra.credit_limit = parseFloat(fields.creditLimit);
      if (fields.accountType) extra.account_type = fields.accountType;
      if (fields.network) extra.network = fields.network;
      if (fields.status) extra.status = fields.status;
      if (fields.upiVpa) extra.upi_vpa = fields.upiVpa;
      if (fields.rewardsSummary) extra.rewards_summary = fields.rewardsSummary;
      if (fields.instrumentType) extra.instrument_type = fields.instrumentType;
      if (fields.issuerName) extra.issuer_name = fields.issuerName;
      if (fields.maskedIdentifier) extra.masked_identifier = fields.maskedIdentifier;
      if (fields.statementDueDate) extra.statement_due_date = fields.statementDueDate;
      if (fields.minimumDue) extra.minimum_due = parseFloat(fields.minimumDue);

      await API.instruments.update(
        inst.id,
        fields.fullIdentifier || undefined,
        fields.billingCycleDay ? parseInt(fields.billingCycleDay, 10) : undefined,
        fields.bankIfsc || undefined,
        extra
      );
      setShowSavedConfirm(true);
      setTimeout(() => setShowSavedConfirm(false), 3000);
      queryClient.invalidateQueries({ queryKey: queryKeys.instruments.detail(inst.id) });
      queryClient.invalidateQueries({ queryKey: queryKeys.instruments.all() });
    } catch (err) {
      toast({ variant: 'destructive', ...getErrorToast(err) });
    } finally {
      setIsSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!inst) return;
    const confirmed = await confirmAction(
      `Delete ${inst.masked_identifier}? This cannot be undone.`,
      'Delete Instrument'
    );
    if (!confirmed) return;
    setIsDeleting(true);
    try {
      await API.instruments.delete(inst.id);
      toast({ title: 'Instrument deleted' });
      if (onClose) onClose();
      queryClient.invalidateQueries({ queryKey: queryKeys.instruments.all() });
    } catch (err) {
      toast({ variant: 'destructive', ...getErrorToast(err) });
    } finally {
      setIsDeleting(false);
    }
  };

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
    recentTransactions,
    totalTxCount,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isTxLoading,
    instrumentStatements,
    instrumentPasswords,
    handleSave,
    handleDelete,
  };
}
