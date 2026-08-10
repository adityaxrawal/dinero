/**
 * History of previous cleanup runs, each still revertible.
 */
import { History } from 'lucide-react';
import type { MerchantCleanupRun } from '@/lib/ipc';
import RunHistoryRow from './RunHistoryRow';

/** History of previous runs, each still revertible. */
export default function PastRuns({
  runs,
  onUndoRun,
  onUndoChange,
  busyId,
}: {
  runs: MerchantCleanupRun[];
  onUndoRun: (run: MerchantCleanupRun) => void;
  onUndoChange: (correctionId: string) => void;
  busyId: string | null;
}) {
  return (
    <div className="mt-6 pt-6 border-t border-[#064E3B]/10">
      <h3 className="font-bold text-[15px] text-[#064E3B] flex items-center gap-2">
        <History className="w-4 h-4" /> Past runs
      </h3>
      <p className="text-[13px] mt-1 mb-3 text-[#064E3B]/65 leading-relaxed max-w-2xl">
        Every run stays undoable — as a whole or one merchant at a time. Undoing also retires the
        extraction rules that run taught.
      </p>
      <div className="flex flex-col gap-2">
        {runs.map((r) => (
          <RunHistoryRow
            key={r.run_id}
            run={r}
            onUndoRun={onUndoRun}
            onUndoChange={onUndoChange}
            busyId={busyId}
          />
        ))}
      </div>
    </div>
  );
}
