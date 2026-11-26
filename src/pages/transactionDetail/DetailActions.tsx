/**
 * Actions available on the transaction detail page.
 */
import { Save, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface DetailActionsProps {
  isDirty: boolean;
  isSaving: boolean;
  isDeleting: boolean;
  showSavedConfirmation: boolean;
  onSave: () => void;
  onDelete: () => void;
  onViewSource: () => void;
}

/** Actions available on the transaction detail page. */
export default function DetailActions({
  isDirty,
  isSaving,
  isDeleting,
  showSavedConfirmation,
  onSave,
  onDelete,
  onViewSource,
}: DetailActionsProps) {
  return (
    <div className="flex flex-col gap-2 pt-2">
      <Button
        className={cn(
          'w-full h-9 font-bold rounded-xl transition-all',
          isDirty
            ? 'bg-emerald-ink hover:bg-emerald-ink/90 text-champagne shadow-md ring-2 ring-emerald-ink/30'
            : 'bg-emerald-ink/40 text-champagne/70 cursor-not-allowed'
        )}
        onClick={onSave}
        disabled={isSaving || (!isDirty && !showSavedConfirmation)}
      >
        {isSaving ? (
          'Saving...'
        ) : (
          <>
            <Save className="w-4 h-4 mr-2" /> Save Changes
          </>
        )}
      </Button>
      <div className="flex gap-2">
        <Button
          variant="outline"
          className="flex-1 h-9 text-red-700 border-red-500/20 bg-red-500/10 hover:bg-red-500/20 font-semibold rounded-xl"
          onClick={onDelete}
          disabled={isDeleting}
        >
          <Trash2 className="w-4 h-4 mr-2" /> {isDeleting ? 'Deleting...' : 'Delete Transaction'}
        </Button>
        <Button
          variant="outline"
          className="flex-1 h-9 border-emerald-ink/20 hover:bg-emerald-ink/10 text-emerald-ink font-semibold rounded-xl"
          onClick={onViewSource}
        >
          View Raw Source
        </Button>
      </div>
    </div>
  );
}
