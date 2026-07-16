import { useState } from 'react';
import { Tag as TagIcon, Trash2, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import type { CategoryRecord } from '@/lib/ipc';
import { useUpdateTransactionCategory } from '@/hooks/mutations/useUpdateTransactionCategory';
import { useAddTransactionTag } from '@/hooks/mutations/useAddTransactionTag';
import { useSoftDeleteTransaction } from '@/hooks/mutations/useSoftDeleteTransaction';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage } from '@/lib/getErrorMessage';

interface TransactionQuickActionsProps {
  transactionId: string;
  currentCategoryId: string;
  categories: CategoryRecord[];
}

/**
 * TASK-FE-009 (Doc 30): inline quick actions (category change, tag add,
 * soft-delete with confirmation), applied optimistically then reconciled
 * with the real IPC response via the `useUpdateTransactionCategory`/
 * `useAddTransactionTag`/`useSoftDeleteTransaction` mutation hooks.
 */
export default function TransactionQuickActions({
  transactionId,
  currentCategoryId,
  categories,
}: TransactionQuickActionsProps) {
  const { toast } = useToast();
  const [tagInputOpen, setTagInputOpen] = useState(false);
  const [tagValue, setTagValue] = useState('');

  const updateCategory = useUpdateTransactionCategory();
  const addTag = useAddTransactionTag();
  const softDelete = useSoftDeleteTransaction();

  const handleCategoryChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const categoryId = e.target.value;
    if (!categoryId || categoryId === currentCategoryId) return;
    updateCategory.mutate(
      { transactionId, categoryId },
      { onError: (err) => toast({ variant: 'destructive', title: 'Category change failed', description: getErrorMessage(err) }) },
    );
  };

  const handleAddTag = () => {
    const tagName = tagValue.trim();
    if (!tagName) return;
    addTag.mutate(
      { transactionId, tagName },
      {
        onSuccess: () => toast({ title: 'Tag added' }),
        onError: (err) => toast({ variant: 'destructive', title: 'Add tag failed', description: getErrorMessage(err) }),
      },
    );
    setTagValue('');
    setTagInputOpen(false);
  };

  const handleDelete = async (e: React.MouseEvent) => {
    e.stopPropagation();
    let confirmed: boolean;
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask('Delete this transaction? This cannot be undone.', { title: 'Delete Transaction', kind: 'warning' });
    } catch {
      confirmed = confirm('Delete this transaction? This cannot be undone.');
    }
    if (!confirmed) return;
    softDelete.mutate(transactionId, {
      onSuccess: () => toast({ title: 'Transaction deleted' }),
      onError: (err) =>
        toast({ variant: 'destructive', title: 'Delete failed', description: getErrorMessage(err, 'Only manually-entered transactions can be deleted.') }),
    });
  };

  return (
    <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
      <select
        aria-label="Change category"
        className="text-xs border border-border rounded-md bg-background px-1 py-0.5 max-w-[90px]"
        value={currentCategoryId}
        onChange={handleCategoryChange}
        disabled={updateCategory.isPending}
      >
        {categories.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>

      {tagInputOpen ? (
        <Input
          autoFocus
          aria-label="New tag name"
          className="h-6 w-20 text-xs px-1"
          value={tagValue}
          onChange={(e) => setTagValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleAddTag()}
          onBlur={() => setTagInputOpen(false)}
        />
      ) : (
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6"
          aria-label="Add tag"
          onClick={() => setTagInputOpen(true)}
          disabled={addTag.isPending}
        >
          {addTag.isPending ? <Loader2 className="w-3 h-3 animate-spin" /> : <TagIcon className="w-3 h-3" />}
        </Button>
      )}

      <Button
        variant="ghost"
        size="icon"
        className="h-6 w-6 text-red-700 hover:text-red-700"
        aria-label="Delete transaction"
        onClick={handleDelete}
        disabled={softDelete.isPending}
      >
        {softDelete.isPending ? <Loader2 className="w-3 h-3 animate-spin" /> : <Trash2 className="w-3 h-3" />}
      </Button>
    </div>
  );
}
