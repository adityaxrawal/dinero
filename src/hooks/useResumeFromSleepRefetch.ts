import { useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { queryKeys } from '@/lib/queryKeys';

/**
 * Refresh ledger data after the machine wakes from sleep.
 *
 * There is no wake event available to the webview, so this infers one from
 * document visibility: the window going hidden and returning much later is the
 * observable signature of a sleep. The threshold is what separates that from an
 * ordinary window switch, which needs no refetch.
 *
 * Only transactions and dashboard data are invalidated -- those are what the
 * backend may have changed while the frontend was suspended.
 */
// Hidden for at least this long is treated as a sleep rather than a brief
// window switch. Exported so tests can drive the boundary directly.
export const SLEEP_GAP_THRESHOLD_MS = 60_000;

/** Refreshes ledger data after the machine wakes from sleep. */
export function useResumeFromSleepRefetch(): void {
  const queryClient = useQueryClient();
  const hiddenAtRef = useRef<number | null>(null);

  useEffect(() => {
    /**
     * Infers a sleep from the window being hidden and returning much later.
     *
     * There is no wake event available to the webview, so visibility plus elapsed
     * time is the observable signature.
     */
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        hiddenAtRef.current = Date.now();
        return;
      }

      const hiddenAt = hiddenAtRef.current;
      hiddenAtRef.current = null;
      if (hiddenAt == null) return;

      const gap = Date.now() - hiddenAt;
      if (gap >= SLEEP_GAP_THRESHOLD_MS) {
        queryClient.invalidateQueries({ queryKey: queryKeys.transactions.all() });
        queryClient.invalidateQueries({ queryKey: queryKeys.dashboard.all() });
      }
    };

    document.addEventListener('visibilitychange', handleVisibilityChange);
    return () => document.removeEventListener('visibilitychange', handleVisibilityChange);
  }, [queryClient]);
}
