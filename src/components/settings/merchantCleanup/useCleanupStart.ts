/**
 * Starts a cleanup run and tracks its pending state.
 */
import { useState } from 'react';
import { API, type MerchantCleanupPreview, type MerchantCleanupProgress } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';
import type { FeedEntry } from './format';

interface UseCleanupStartArgs {
  preview: MerchantCleanupPreview | null;
  setError: (message: string | null) => void;
  setProgress: (progress: MerchantCleanupProgress | null) => void;
  setFeed: (feed: FeedEntry[]) => void;
  setStartedAt: (at: number | null) => void;
}

/** Starts a run and tracks its pending state. */
export function useCleanupStart({
  preview,
  setError,
  setProgress,
  setFeed,
  setStartedAt,
}: UseCleanupStartArgs) {
  const [isStarting, setIsStarting] = useState(false);

  /** Begins the run. */
  const handleStart = async () => {
    setError(null);
    setIsStarting(true);
    setFeed([]);
    setStartedAt(Date.now());
    try {
      const runId = await API.merchantCleanup.start();
      setProgress({
        run_id: runId,
        processed: 0,
        total: preview?.candidate_count ?? 0,
        applied: 0,
        skipped: 0,
        current_merchant: null,
        bank_name: null,
        resolved_merchant: null,
        resolved_category: null,
        status: 'running',
      });
    } catch (err: unknown) {
      setError(errorMessage(err));
      setStartedAt(null);
    } finally {
      setIsStarting(false);
    }
  };

  /** Requests cancellation. */
  const handleCancel = async () => {
    try {
      await API.merchantCleanup.cancel();
    } catch (err: unknown) {
      setError(errorMessage(err));
    }
  };

  return { isStarting, handleStart, handleCancel };
}
