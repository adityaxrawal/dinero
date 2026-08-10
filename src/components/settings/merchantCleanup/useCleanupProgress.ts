/**
 * Subscribes to live progress events for a running cleanup.
 */
import { useState, useRef, useMemo } from 'react';
import type { MerchantCleanupProgress } from '@/lib/ipc';
import { useIpcListen } from '@/hooks/useIpcListen';
import { useNowTicker } from '@/hooks/useNowTicker';
import { formatClock, formatDuration, FEED_LENGTH, type FeedEntry } from './format';

/** Subscribes to live progress events for a running cleanup. */
export function useCleanupProgress({ running, onRunEnd }: { running: boolean; onRunEnd: () => void }) {
  const [progress, setProgress] = useState<MerchantCleanupProgress | null>(null);
  const [feed, setFeed] = useState<FeedEntry[]>([]);
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const feedKey = useRef(0);

  useIpcListen<MerchantCleanupProgress>('merchant_cleanup_progress', (payload) => {
    setProgress(payload);
    if (payload.status === 'running') {
      setStartedAt((prev) => prev ?? Date.now());
    }
    if (payload.current_merchant) {
      feedKey.current += 1;
      const entry: FeedEntry = {
        key: feedKey.current,
        before: payload.current_merchant,
        after: payload.resolved_merchant,
        category: payload.resolved_category,
      };
      setFeed((prev) => [entry, ...prev].slice(0, FEED_LENGTH));
    }
    if (payload.status !== 'running') {
      setStartedAt(null);
      onRunEnd();
    }
  });

  const isRunning = progress?.status === 'running' || running;
  const now = useNowTicker(isRunning);

  const live = useMemo(() => {
    if (!progress || startedAt === null) return null;
    const elapsedMs = now - startedAt;
    const perMin = elapsedMs > 0 ? (progress.processed / elapsedMs) * 60000 : 0;
    const remaining = progress.total - progress.processed;
    return {
      elapsed: formatClock(elapsedMs),
      perMin: perMin >= 0.1 ? perMin.toFixed(1) : '—',
      eta: perMin > 0 ? formatDuration((remaining / perMin) * 60) : '—',
    };
  }, [progress, now, startedAt]);

  const pct =
    progress && progress.total > 0 ? Math.round((progress.processed / progress.total) * 100) : 0;
  const isFinished = progress !== null && progress.status !== 'running' && progress.processed > 0;

  return { progress, setProgress, feed, setFeed, setStartedAt, isRunning, live, pct, isFinished };
}
