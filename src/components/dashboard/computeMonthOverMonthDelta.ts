/**
 * Derives the month-over-month change from the spend trend series.
 *
 * Computed from the last two monthly points rather than a dedicated backend
 * field. Returns null when there is no full pair to compare, so a new install
 * shows nothing instead of a fabricated figure.
 */
import type { SpendTrendPoint } from '@/lib/ipc';

/**
 * Month-over-month change, derived from the last two monthly trend points.
 *
 * Returns null without a full pair to compare, so a new install shows nothing
 * rather than a fabricated figure.
 */
export function computeMonthOverMonthDelta(trend: SpendTrendPoint[] | undefined): number | null {
  if (!trend || trend.length < 2) return null;
  const sorted = [...trend].sort((a, b) => a.period.localeCompare(b.period));
  const current = sorted[sorted.length - 1].total_spend;
  const previous = sorted[sorted.length - 2].total_spend;
  if (previous <= 0) return null;
  return ((current - previous) / previous) * 100;
}
