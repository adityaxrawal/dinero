import { useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { queryKeys } from '@/lib/queryKeys';

/**
 * TASK-RT-005 (Doc 30): "On resume-from-sleep, triggers an explicit
 * one-time full refetch (in addition to the recovery-sync's own catch-up)
 * so anything missed during sleep is reflected immediately rather than
 * waiting for the next natural poll cycle."
 *
 * No native macOS sleep/wake notification exists anywhere in this codebase
 * to hook into, so this uses `document.visibilitychange` as the practical
 * signal a single-window Tauri app has available: the window reliably goes
 * `hidden` when the Mac sleeps or the app is backgrounded, and `visible`
 * again on wake/foreground. A short gap (an accidental double-click,
 * alt-tabbing briefly) is deliberately not treated as "resumed from
 * sleep" -- only a gap past `SLEEP_GAP_THRESHOLD_MS` triggers the refetch,
 * so this doesn't fire on every ordinary window blur/focus.
 */
export const SLEEP_GAP_THRESHOLD_MS = 60_000;

export function useResumeFromSleepRefetch(): void {
  const queryClient = useQueryClient();
  const hiddenAtRef = useRef<number | null>(null);

  useEffect(() => {
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
