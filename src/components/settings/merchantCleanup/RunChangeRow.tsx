/**
 * A single applied correction within a run, with its undo control.
 */
import { ArrowRight } from 'lucide-react';
import type { MerchantCleanupRun } from '@/lib/ipc';
import { cn } from '@/lib/utils';

type Change = MerchantCleanupRun['changes'][number];

/** One applied correction, with its undo control. */
export default function RunChangeRow({
  change,
  onUndoChange,
  busyId,
}: {
  change: Change;
  onUndoChange: (correctionId: string) => void;
  busyId: string | null;
}) {
  const isBusy = busyId === change.correction_id;

  return (
    <li
      className={cn(
        'px-4 py-2.5 flex items-center justify-between gap-3',
        change.reverted && 'opacity-50'
      )}
    >
      <div className="min-w-0 flex items-center gap-2 flex-wrap text-[12px]">
        <span className="font-mono text-emerald-ink/85 line-through decoration-emerald-ink/30">
          {change.previous_merchant ?? '—'}
        </span>
        <ArrowRight className="w-3 h-3 shrink-0 text-emerald-ink/80" />
        <span className="font-semibold text-emerald-ink">{change.new_merchant ?? '—'}</span>
        {change.category && (
          <span className="text-[11px] font-semibold px-1.5 py-0.5 rounded bg-emerald-ink/[0.07] text-emerald-ink/85">
            {change.category}
          </span>
        )}
      </div>
      {change.reverted ? (
        <span className="text-[11px] text-emerald-ink/80 shrink-0">undone</span>
      ) : (
        <button
          type="button"
          onClick={() => onUndoChange(change.correction_id)}
          disabled={isBusy}
          className="shrink-0 text-[11px] font-semibold text-emerald-ink/85 hover:text-emerald-ink underline underline-offset-2"
        >
          {isBusy ? 'undoing…' : 'undo'}
        </button>
      )}
    </li>
  );
}
