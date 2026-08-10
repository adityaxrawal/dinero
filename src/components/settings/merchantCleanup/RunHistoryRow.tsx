/**
 * One past run in the history list.
 */
import { useState } from 'react';
import { Loader2, Undo2, ChevronRight } from 'lucide-react';
import type { MerchantCleanupRun } from '@/lib/ipc';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { RelativeDate } from '../SettingsPrimitives';
import RunChangeRow from './RunChangeRow';

/** One past run in the history list. */
export default function RunHistoryRow({
  run,
  onUndoRun,
  onUndoChange,
  busyId,
}: {
  run: MerchantCleanupRun;
  onUndoRun: (run: MerchantCleanupRun) => void;
  onUndoChange: (correctionId: string) => void;
  busyId: string | null;
}) {
  const [open, setOpen] = useState(false);
  const isBusy = busyId === run.run_id;

  return (
    <div className="rounded-xl border border-[#064E3B]/10 bg-white overflow-hidden">
      <div className="px-4 py-3 flex items-center gap-3 flex-wrap">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="flex items-center gap-2 min-w-0 flex-1 text-left"
        >
          <ChevronRight
            className={cn(
              'w-4 h-4 shrink-0 text-[#064E3B]/50 transition-transform',
              open && 'rotate-90'
            )}
          />
          <span className="font-semibold text-[13px] text-[#064E3B] shrink-0">
            <RelativeDate iso={run.started_at} />
          </span>
          <span className="text-[12px] text-[#064E3B]/60 truncate">
            {run.applied} still applied
            {run.reverted > 0 && ` · ${run.reverted} undone`}
            {run.banks.length > 0 && ` · ${run.banks.join(', ')}`}
          </span>
        </button>

        {run.applied > 0 ? (
          <Button
            variant="outline"
            size="sm"
            onClick={() => onUndoRun(run)}
            disabled={isBusy}
            className="shrink-0 border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
          >
            {isBusy ? (
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
            ) : (
              <Undo2 className="w-3.5 h-3.5" />
            )}
            <span className="ml-1.5">Undo run</span>
          </Button>
        ) : (
          <span className="text-[11px] font-semibold uppercase tracking-wide text-[#064E3B]/40 shrink-0">
            Already undone
          </span>
        )}
      </div>

      {open && (
        <ul className="border-t border-[#064E3B]/[0.07] divide-y divide-[#064E3B]/[0.07]">
          {run.changes.map((c) => (
            <RunChangeRow
              key={c.correction_id}
              change={c}
              onUndoChange={onUndoChange}
              busyId={busyId}
            />
          ))}
        </ul>
      )}
    </div>
  );
}
