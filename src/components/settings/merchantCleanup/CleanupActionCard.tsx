/**
 * Card framing the cleanup action and its current state.
 */
import type {
  LlmModelInfo,
  MerchantCleanupPreview,
  MerchantCleanupProgress,
  MerchantCleanupRun,
} from '@/lib/ipc';
import { cn } from '@/lib/utils';
import type { FeedEntry } from './format';
import { headlineTitle, headlineBlurb } from './headline';
import CleanupActionButtons from './CleanupActionButtons';
import CleanupRunProgress from './CleanupRunProgress';
import WorstMatchRow from './WorstMatchRow';

interface LiveStats {
  elapsed: string;
  perMin: string;
  eta: string;
}

export interface RunState {
  progress: MerchantCleanupProgress | null;
  feed: FeedEntry[];
  live: LiveStats | null;
  pct: number;
  isRunning: boolean;
  isFinished: boolean;
  isStarting: boolean;
  finishedRun: MerchantCleanupRun | undefined;
  busyId: string | null;
}

interface CleanupActionCardProps {
  preview: MerchantCleanupPreview | null;
  activeModel: LlmModelInfo | null;
  noModel: boolean;
  run: RunState;
  onStart: () => void;
  onCancel: () => void;
  onUndoRun: (run: MerchantCleanupRun) => void;
}

/** Card framing the cleanup action and its current state. */
export default function CleanupActionCard({
  preview,
  activeModel,
  noModel,
  run,
  onStart,
  onCancel,
  onUndoRun,
}: CleanupActionCardProps) {
  const { progress, isRunning, isFinished } = run;
  const state = { preview, progress, isRunning, isFinished };
  const worst = preview?.samples[0];

  return (
    <div
      className={cn(
        'mb-5 p-5 rounded-xl border transition-colors',
        isRunning ? 'bg-[#064E3B]/[0.06] border-[#064E3B]/30' : 'bg-[#F8E7C9]/50 border-[#064E3B]/10'
      )}
    >
      <div className="flex items-start justify-between flex-wrap gap-3">
        <div className="min-w-0">
          <h3 className="font-bold text-[15px] text-[#064E3B]">{headlineTitle(state)}</h3>
          <p className="text-[12px] mt-1 text-[#064E3B]/60 leading-relaxed max-w-xl">
            {headlineBlurb(state)}
          </p>
        </div>

        <CleanupActionButtons
          preview={preview}
          isRunning={isRunning}
          isStarting={run.isStarting}
          isFinished={isFinished}
          noModel={noModel}
          finishedRun={run.finishedRun}
          busyId={run.busyId}
          onStart={onStart}
          onCancel={onCancel}
          onUndoRun={onUndoRun}
        />
      </div>

      {!isRunning && !isFinished && worst && <WorstMatchRow sample={worst} />}

      {progress && (isRunning || isFinished) && (
        <CleanupRunProgress
          progress={progress}
          pct={run.pct}
          live={run.live}
          isRunning={isRunning}
          activeModel={activeModel}
          feed={run.feed}
        />
      )}
    </div>
  );
}
