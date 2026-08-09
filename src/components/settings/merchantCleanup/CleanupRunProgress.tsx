import type { MerchantCleanupProgress, LlmModelInfo } from '@/lib/ipc';
import type { FeedEntry } from './format';
import CleanupProgressBar from './CleanupProgressBar';
import CleanupCounters from './CleanupCounters';
import CleanupFeed from './CleanupFeed';

interface LiveStats {
  elapsed: string;
  perMin: string;
  eta: string;
}

export default function CleanupRunProgress({
  progress,
  pct,
  live,
  isRunning,
  activeModel,
  feed,
}: {
  progress: MerchantCleanupProgress;
  pct: number;
  live: LiveStats | null;
  isRunning: boolean;
  activeModel: LlmModelInfo | null;
  feed: FeedEntry[];
}) {
  return (
    <div className="mt-4">
      <CleanupProgressBar progress={progress} pct={pct} />
      <CleanupCounters progress={progress} live={live} isRunning={isRunning} />

      {isRunning && (
        <p className="mt-2 text-[11px] text-[#064E3B]/50">
          {live && <>elapsed {live.elapsed} · </>}
          {activeModel?.name ?? 'on-device model'} · nothing leaves your Mac
        </p>
      )}

      <CleanupFeed feed={feed} />
    </div>
  );
}
