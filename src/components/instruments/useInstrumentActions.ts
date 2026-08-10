/**
 * Create, update and delete actions for instruments, with confirmation and toasts.
 */
import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { API, type InstrumentRecord } from '@/lib/ipc';
import { getErrorToast } from '@/lib/errorMapping';
import { useToast } from '@/hooks/use-toast';
import { confirmAction } from '@/lib/confirmDialog';
import { queryKeys } from '@/lib/queryKeys';
import { buildInstrumentUpdate, type InstrumentFormFields } from './instrumentUpdate';

/** Create, update and delete actions with confirmation and toasts. */
export function useInstrumentActions(
  inst: InstrumentRecord | undefined,
  fields: InstrumentFormFields,
  onClose?: () => void
) {
  const { toast } = useToast();
  const queryClient = useQueryClient();
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [showSavedConfirm, setShowSavedConfirm] = useState(false);

  /** Surfaces a failure as a toast. */
  const reportError = (err: unknown) => toast({ variant: 'destructive', ...getErrorToast(err) });

  /** Persists edited fields. */
  const handleSave = async () => {
    if (!inst) return;
    setIsSaving(true);
    try {
      await API.instruments.update(
        inst.id,
        fields.fullIdentifier || undefined,
        fields.billingCycleDay ? parseInt(fields.billingCycleDay, 10) : undefined,
        fields.bankIfsc || undefined,
        buildInstrumentUpdate(fields)
      );
      setShowSavedConfirm(true);
      setTimeout(() => setShowSavedConfirm(false), 3000);
      queryClient.invalidateQueries({ queryKey: queryKeys.instruments.detail(inst.id) });
      queryClient.invalidateQueries({ queryKey: queryKeys.instruments.all() });
    } catch (err) {
      reportError(err);
    } finally {
      setIsSaving(false);
    }
  };

  /** Soft-deletes the instrument after confirmation. */
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
      reportError(err);
    } finally {
      setIsDeleting(false);
    }
  };

  return {
    isSaving,
    isDeleting,
    showSavedConfirm,
    setShowSavedConfirm,
    handleSave,
    handleDelete,
  };
}
