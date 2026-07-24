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
  const { data: txPage } = useTransactionsInfiniteList(
    instrumentId ? { instrument_id: instrumentId } : {}
  );
  const { data: statements = [] } = useStatementsList();
  const { data: pdfPasswords = [] } = usePdfPasswordsList();
  const forgetPassword = useForgetPdfPassword();

  const [fullIdentifier, setFullIdentifier] = useState('');
  const [billingCycleDay, setBillingCycleDay] = useState('');
  const [bankIfsc, setBankIfsc] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [showSavedConfirm, setShowSavedConfirm] = useState(false);

  useEffect(() => {
    if (detailInst) {
      setFullIdentifier(detailInst.full_identifier ?? '');
      setBillingCycleDay(detailInst.billing_cycle_day?.toString() ?? '');
      setBankIfsc(detailInst.bank_ifsc ?? '');
      setShowSavedConfirm(false);
    }
  }, [detailInst]);

  const inst = detailInst ?? initialInstrument;
  const isNegative = (inst?.current_balance ?? 0) < 0;

  const recentTransactions = txPage?.pages[0]?.records.slice(0, 10) ?? [];
  const instrumentStatements = inst ? statements.filter((s) => s.instrument_id === inst.id) : [];
  const instrumentPasswords = inst ? pdfPasswords.filter((p) => p.instrument_id === inst.id) : [];

  const handleSave = async () => {
    if (!inst) return;
    setIsSaving(true);
    try {
      await API.instruments.update(
        inst.id,
        fullIdentifier || undefined,
        billingCycleDay ? parseInt(billingCycleDay, 10) : undefined,
        bankIfsc || undefined
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
    fullIdentifier,
    setFullIdentifier,
    billingCycleDay,
    setBillingCycleDay,
    bankIfsc,
    setBankIfsc,
    isSaving,
    isDeleting,
    showSavedConfirm,
    isNegative,
    recentTransactions,
    instrumentStatements,
    instrumentPasswords,
    handleSave,
    handleDelete,
  };
}
