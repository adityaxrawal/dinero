/**
 * Progress bar for an in-flight cleanup run.
 */
import type { MerchantCleanupProgress } from '@/lib/ipc';
import { cn } from '@/lib/utils';

const BAR_TONE: Record<string, string> = {
  failed: 'bg-red-500',
  cancelled: 'bg-amber-500',
};

/** Progress bar for an in-flight run. */
export default function CleanupProgressBar({
  progress,
  pct,
}: {
  progress: MerchantCleanupProgress;
  pct: number;
}) {
  return (
    <>
      <div className="h-1.5 rounded-full bg-emerald-ink/10 overflow-hidden">
        <div
          className={cn(
            'h-full rounded-full transition-all duration-300',
            BAR_TONE[progress.status] ?? 'bg-emerald-ink'
          )}
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="flex items-center justify-between mt-1.5 text-[11px] font-semibold text-emerald-ink/60 tabular-nums">
        <span>
          {progress.processed} / {progress.total}
        </span>
        <span>{pct}%</span>
      </div>
    </>
  );
}
