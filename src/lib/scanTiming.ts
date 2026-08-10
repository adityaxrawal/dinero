/**
 * Duration formatting and completion estimates for long-running mailbox scans.
 *
 * A historical scan can run for minutes or hours, so the progress UI needs both
 * a readable elapsed time and a projected time remaining. Both helpers are pure
 * and take their inputs explicitly, which keeps them directly testable and free
 * of any dependency on wall-clock time.
 */

/**
 * Renders a second count as a compact human duration.
 *
 * Precision drops as the magnitude grows -- seconds below a minute, minutes and
 * seconds below an hour, hours and minutes beyond that -- because trailing
 * seconds are noise once a scan has been running for an hour.
 */
export function formatDuration(totalSeconds: number): string {
  // Clamped at zero so a negative input (clock skew, a stale timestamp) can
  // never render as a negative duration.
  const seconds = Math.max(0, Math.round(totalSeconds));
  if (seconds < 60) return `${seconds}s`;
  const totalMinutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  if (totalMinutes < 60) return `${totalMinutes}m ${remainingSeconds}s`;
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return `${hours}h ${minutes}m`;
}

/**
 * Projects the seconds remaining in a scan from the throughput observed so far.
 *
 * Returns null rather than a number whenever an estimate would be meaningless:
 * before any item has been processed there is no rate to extrapolate from, and
 * once processed has reached total the work is finished. Callers are expected
 * to treat null as "no estimate yet" and show nothing.
 */
export function estimateEtaSeconds(
  processed: number,
  total: number,
  elapsedSeconds: number
): number | null {
  if (processed <= 0 || total <= 0 || processed >= total) return null;

  // Simple linear extrapolation of the average rate to date. Throughput is not
  // actually uniform across a scan, so this drifts early and tightens as more
  // of the mailbox is processed and the average stabilises.
  const secondsPerItem = elapsedSeconds / processed;
  return Math.round(secondsPerItem * (total - processed));
}
