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
            ? 'bg-[#064E3B] hover:bg-[#064E3B]/90 text-[#F8E7C9] shadow-md ring-2 ring-[#064E3B]/30'
            : 'bg-[#064E3B]/40 text-[#F8E7C9]/70 cursor-not-allowed'
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
          className="flex-1 h-9 border-[#064E3B]/20 hover:bg-[#064E3B]/10 text-[#064E3B] font-semibold rounded-xl"
          onClick={onViewSource}
        >
          View Raw Source
        </Button>
      </div>
    </div>
  );
}
