/**
 * Queue of statements that failed to process, with retry and discard.
 */
import { AlertTriangle, CheckCircle2, Loader2, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useUnprocessedQueue } from './queue/useUnprocessedQueue';
import { ReviewableSection, ActionableGroups } from './queue/QueueSections';

interface UnprocessedItemsQueueProps {
  onEnterPassword: (statementId: string) => void;
}

const PANEL = 'rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 p-4';

/** Shown when nothing is awaiting action. */
function EmptyQueue() {
  return (
    <div className="rounded-xl border border-[#064E3B]/10 bg-[#F8E7C9]/50 p-10 text-center">
      <CheckCircle2 className="mx-auto mb-3 h-8 w-8 text-[#064E3B]/40" aria-hidden="true" />
      <p className="text-sm font-medium text-[#064E3B]">Nothing needs your attention</p>
      <p className="mt-1 text-[13px] text-[#064E3B]/60">
        Statements that need a password or fail to parse will collect here.
      </p>
    </div>
  );
}

/** Progress for a bulk re-parse. */
function ReparseProgress({ processed, total }: { processed: number; total: number }) {
  const percent = total > 0 ? Math.round((processed / total) * 100) : 0;
  return (
    <div className={PANEL}>
      <div
        className="h-1.5 w-full overflow-hidden rounded-full bg-[#064E3B]/10"
        role="progressbar"
        aria-valuenow={percent}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Re-parse progress"
      >
        <div
          className="h-full rounded-full bg-[#064E3B] transition-all duration-300"
          style={{ width: `${percent}%` }}
        />
      </div>
      <p className="mt-2 text-[13px] text-[#064E3B]/70">
        {processed} of {total} checked
      </p>
    </div>
  );
}

/** Queue of statements that failed, with retry and discard. */
export default function UnprocessedItemsQueue({ onEnterPassword }: UnprocessedItemsQueueProps) {
  const queue = useUnprocessedQueue(onEnterPassword);
  const { total, progress, isReparsing } = queue;

  if (!queue.groups || total === 0) return <EmptyQueue />;

  return (
    <div className="space-y-4">
      <div className={`flex flex-wrap items-center justify-between gap-3 ${PANEL}`}>
        <div className="min-w-0">
          <p className="flex items-center gap-2 text-sm font-semibold text-[#064E3B]">
            <AlertTriangle className="h-4 w-4 text-amber-600" aria-hidden="true" />
            {total} {total === 1 ? 'statement needs' : 'statements need'} attention
          </p>
          <p className="mt-0.5 text-[13px] text-[#064E3B]/60">
            Re-parsing retries every one of them against your stored passwords.
          </p>
        </div>
        <Button
          onClick={queue.handleReparseAll}
          disabled={isReparsing || queue.actionable === 0}
          className="shrink-0 bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90"
        >
          {isReparsing ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden="true" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" aria-hidden="true" />
          )}
          {isReparsing ? 'Re-parsing…' : 'Re-parse All'}
        </Button>
      </div>

      {progress && !progress.done && (
        <ReparseProgress processed={progress.processed} total={progress.total} />
      )}

      {queue.reviewable.length > 0 && <ReviewableSection items={queue.reviewable} />}

      <ActionableGroups queue={queue} />
    </div>
  );
}
