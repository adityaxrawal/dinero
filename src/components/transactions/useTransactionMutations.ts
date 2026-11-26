/**
 * Save, delete and tag mutations for a transaction, with feedback.
 */
import { useToast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';
import { useUpdateTransactionFields } from '@/hooks/mutations/useUpdateTransactionFields';
import { useAddTransactionTag } from '@/hooks/mutations/useAddTransactionTag';
import { useRemoveTransactionTag } from '@/hooks/mutations/useRemoveTransactionTag';
import { useSoftDeleteTransaction } from '@/hooks/mutations/useSoftDeleteTransaction';
import { API } from '@/lib/ipc';
import type { TransactionFields } from './transactionFormFields';

interface UseTransactionMutationsArgs {
  transactionId: string | undefined;
  fields: TransactionFields;
  tags: string[];
  newTag: string;
  setNewTag: (value: string) => void;
  setShowSavedConfirm: (value: boolean) => void;
  onClose?: (() => void) | undefined;
}

/** Save, delete and tag mutations, with user feedback. */
export function useTransactionMutations({
  transactionId,
  fields,
  tags,
  newTag,
  setNewTag,
  setShowSavedConfirm,
  onClose,
}: UseTransactionMutationsArgs) {
  const { toast } = useToast();
  const updateFields = useUpdateTransactionFields();
  const addTag = useAddTransactionTag();
  const removeTag = useRemoveTransactionTag();
  const softDelete = useSoftDeleteTransaction();

  /** Surfaces a mutation failure as a toast. */
  const reportError = (err: unknown, fallback?: string) =>
    toast({ variant: 'destructive', ...getErrorToast(err, fallback) });

  /** Persists edited fields. */
  const handleSave = () => {
    if (!transactionId) return;
    const parsedAmount = parseFloat(fields.amountStr);
    const amountMinor = !isNaN(parsedAmount) ? Math.round(parsedAmount * 100) : undefined;

    updateFields.mutate(
      {
        transactionId,
        merchantDisplayName: fields.merchant,
        categoryId: fields.categoryId,
        notes: fields.notes,
        amountMinor,
        direction: fields.direction,
        eventTime: fields.eventTime || undefined,
        instrumentId: fields.instrumentId || undefined,
      },
      {
        onSuccess: () => {
          setShowSavedConfirm(true);
          setTimeout(() => setShowSavedConfirm(false), 3000);
        },
        onError: (err) => reportError(err),
      }
    );
  };

  /** Attaches a tag, creating it if new. */
  const handleAddTag = () => {
    const t = newTag.trim();
    if (!t || tags.includes(t) || !transactionId) return;
    addTag.mutate({ transactionId, tagName: t }, { onError: (err) => reportError(err) });
    setNewTag('');
  };

  /** Detaches a tag, leaving the tag itself intact. */
  const handleRemoveTag = (tag: string) => {
    if (!transactionId) return;
    removeTag.mutate({ transactionId, tagName: tag }, { onError: (err) => reportError(err) });
  };

  /**
   * Soft-deletes the transaction, with Undo in the toast. It used to ask for
   * confirmation and say "This cannot be undone", which was untrue (F12-1).
   */
  const handleDelete = async () => {
    if (!transactionId) return;
    const id = transactionId;
    softDelete.mutate(id, {
      onSuccess: () => {
        toast({
          title: 'Transaction deleted',
          actionLabel: 'Undo',
          // The restore emits transaction_created, which refreshes the views.
          onAction: () => {
            API.transactions.restore(id).catch((err) => reportError(err));
          },
        });
        if (onClose) onClose();
      },
      onError: (err) => reportError(err, 'Only manually-entered transactions can be deleted.'),
    });
  };

  return { updateFields, softDelete, handleSave, handleAddTag, handleRemoveTag, handleDelete };
}
