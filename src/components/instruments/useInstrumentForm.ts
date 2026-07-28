import { useState, useEffect } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API, InstrumentRecord } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import { useToast } from '@/hooks/use-toast';
import { confirmDelete } from '@/lib/confirmDialog';
import { queryKeys } from '@/lib/queryKeys';
import { useInstrumentDetail } from '@/hooks/queries/useInstrumentDetail';
import { useTransactionsInfiniteList } from '@/hooks/queries/useTransactionsInfiniteList';
import { useStatementsList } from '@/hooks/queries/useStatementsList';
import { usePdfPasswordsList } from '@/hooks/queries/usePdfPasswordsList';
import { useForgetPdfPassword } from '@/hooks/mutations/useForgetPdfPassword';

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

  const [issuerName, setIssuerName] = useState('');
  const [maskedIdentifier, setMaskedIdentifier] = useState('');
  const [nickname, setNickname] = useState('');
  const [fullIdentifier, setFullIdentifier] = useState('');
  const [billingCycleDay, setBillingCycleDay] = useState('');
  const [bankIfsc, setBankIfsc] = useState('');
  const [instrumentType, setInstrumentType] = useState('credit_card');
  const [status, setStatus] = useState('active');
  const [creditLimit, setCreditLimit] = useState('');
  const [network, setNetwork] = useState('');
  const [accountType, setAccountType] = useState('');
  const [upiVpa, setUpiVpa] = useState('');
  const [rewardsSummary, setRewardsSummary] = useState('');
  const [statementDueDate, setStatementDueDate] = useState('');
  const [minimumDue, setMinimumDue] = useState('');

  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [showSavedConfirm, setShowSavedConfirm] = useState(false);

  useEffect(() => {
    const active = detailInst ?? initialInstrument;
    if (active) {
      setIssuerName(active.issuer_name ?? '');
      setMaskedIdentifier(active.masked_identifier ?? '');
      setNickname(active.nickname ?? '');
      setFullIdentifier(active.full_identifier ?? '');
      setBillingCycleDay(active.billing_cycle_day?.toString() ?? '');
      setBankIfsc(active.bank_ifsc ?? '');
      setInstrumentType(active.instrument_type ?? 'credit_card');
      setStatus(active.status ?? 'active');
      setCreditLimit(active.credit_limit?.toString() ?? '');
      setNetwork(active.network ?? '');
      setAccountType(active.account_type ?? '');
      setUpiVpa(active.upi_vpa ?? '');
      setRewardsSummary(active.rewards_summary ?? '');
      setStatementDueDate(active.statement_due_date ?? '');
      setMinimumDue(active.minimum_due != null ? (active.minimum_due / 100.0).toString() : '');
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
      if (nickname) extra.nickname = nickname;
      if (creditLimit) extra.credit_limit = parseFloat(creditLimit);
      if (accountType) extra.account_type = accountType;
      if (network) extra.network = network;
      if (status) extra.status = status;
      if (upiVpa) extra.upi_vpa = upiVpa;
      if (rewardsSummary) extra.rewards_summary = rewardsSummary;
      if (instrumentType) extra.instrument_type = instrumentType;
      if (issuerName) extra.issuer_name = issuerName;
      if (maskedIdentifier) extra.masked_identifier = maskedIdentifier;
      if (statementDueDate) extra.statement_due_date = statementDueDate;
      if (minimumDue) extra.minimum_due = parseFloat(minimumDue);

      await API.instruments.update(
        inst.id,
        fullIdentifier || undefined,
        billingCycleDay ? parseInt(billingCycleDay, 10) : undefined,
        bankIfsc || undefined,
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
    const confirmed = await confirmDelete(
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
    issuerName,
    setIssuerName,
    maskedIdentifier,
    setMaskedIdentifier,
    nickname,
    setNickname,
    fullIdentifier,
    setFullIdentifier,
    billingCycleDay,
    setBillingCycleDay,
    bankIfsc,
    setBankIfsc,
    instrumentType,
    setInstrumentType,
    status,
    setStatus,
    creditLimit,
    setCreditLimit,
    network,
    setNetwork,
    accountType,
    setAccountType,
    upiVpa,
    setUpiVpa,
    rewardsSummary,
    setRewardsSummary,
    statementDueDate,
    setStatementDueDate,
    minimumDue,
    setMinimumDue,
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
