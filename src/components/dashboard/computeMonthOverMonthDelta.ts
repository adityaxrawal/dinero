import type { SpendTrendPoint } from '@/lib/ipc';

/**
 * TASK-FE-008: `DashboardSummary` has no month-over-month delta field —
 * derived here from the monthly-granularity spend trend series instead
 * (`analytics_spend_trend`'s last two points), rather than fabricating a
 * number the backend doesn't provide. Returns null when there isn't a full
 * pair of months to compare (e.g. a brand-new install) or the prior month
 * had zero spend (percentage change is undefined at that point).
 */
export function computeMonthOverMonthDelta(trend: SpendTrendPoint[] | undefined): number | null {
  if (!trend || trend.length < 2) return null;
  const sorted = [...trend].sort((a, b) => a.period.localeCompare(b.period));
  const current = sorted[sorted.length - 1].total_spend;
  const previous = sorted[sorted.length - 2].total_spend;
  if (previous <= 0) return null;
  return ((current - previous) / previous) * 100;
}
