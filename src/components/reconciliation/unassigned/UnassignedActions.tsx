/**
 * Resolve and dismiss actions for an unassigned transaction.
 */
import { Ban, Save } from 'lucide-react';
import { Button } from '@/components/ui/button';

/** Resolve and dismiss actions. */
export default function UnassignedActions({
  canSubmit,
  isPending,
  onDismiss,
  onSave,
}: {
  canSubmit: boolean;
  isPending: boolean;
  onDismiss: () => void;
  onSave: () => void;
}) {
  return (
    <div className="mt-8 pt-4 border-t border-[#064E3B]/10 flex justify-end gap-3">
      <Button
        variant="outline"
        className="text-[13px] h-9 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5 font-medium"
        onClick={onDismiss}
        disabled={isPending}
      >
        <Ban className="w-3.5 h-3.5 mr-2" /> Not a Transaction
      </Button>
      <Button
        className="text-[13px] h-9 font-medium bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90"
        onClick={onSave}
        disabled={!canSubmit || isPending}
      >
        <Save className="w-3.5 h-3.5 mr-2" /> Save as Transaction
      </Button>
    </div>
  );
}
