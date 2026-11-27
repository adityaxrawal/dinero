/**
 * Resolve and dismiss actions for an unassigned transaction.
 */
import { Ban, RefreshCw, Save } from 'lucide-react';
import { Button } from '@/components/ui/button';

/** Shown when an entry has no stored email to re-check. */
export const NO_EMAIL_TO_RECHECK =
  'No stored email to re-check: this entry came from a statement or the reconciliation engine.';

/** Re-check, resolve and dismiss actions. */
export default function UnassignedActions({
  canSubmit,
  isPending,
  canRecheck,
  onRecheck,
  onDismiss,
  onSave,
}: {
  canSubmit: boolean;
  isPending: boolean;
  canRecheck: boolean;
  onRecheck: () => void;
  onDismiss: () => void;
  onSave: () => void;
}) {
  return (
    <div className="mt-8 pt-4 border-t border-emerald-ink/10 flex justify-end gap-3">
      <Button
        variant="outline"
        className="text-[13px] h-9 border-emerald-ink/20 text-emerald-ink hover:bg-emerald-ink/5 font-medium mr-auto"
        onClick={onRecheck}
        disabled={!canRecheck || isPending}
        title={
          canRecheck ? 'See what your learned patterns would fill in, before anything is booked' : NO_EMAIL_TO_RECHECK
        }
      >
        <RefreshCw className="w-3.5 h-3.5 mr-2" /> Re-check
      </Button>
      <Button
        variant="outline"
        className="text-[13px] h-9 border-emerald-ink/20 text-emerald-ink hover:bg-emerald-ink/5 font-medium"
        onClick={onDismiss}
        disabled={isPending}
      >
        <Ban className="w-3.5 h-3.5 mr-2" /> Not a Transaction
      </Button>
      <Button
        className="text-[13px] h-9 font-medium bg-emerald-ink text-champagne hover:bg-emerald-ink/90"
        onClick={onSave}
        disabled={!canSubmit || isPending}
      >
        <Save className="w-3.5 h-3.5 mr-2" /> Save as Transaction
      </Button>
    </div>
  );
}
