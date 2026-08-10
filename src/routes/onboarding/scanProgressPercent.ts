/**
 * Percentage complete for the onboarding scan progress bar.
 *
 * Guards against a zero total, which is the normal state before the backend has
 * finished counting the mailbox -- dividing there would yield NaN and render an
 * empty bar rather than a zeroed one.
 */
export function scanProgressPercent(processed: number, total: number): number {
  if (total <= 0) return 0;
  return Math.round((processed / total) * 100);
}
