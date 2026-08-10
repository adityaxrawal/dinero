/**
 * Loads and combines the queries backing the dashboard.
 */
import { useState } from 'react';
import { useDashboardSummary } from '@/hooks/queries/useDashboardSummary';
import { useTransactionsList } from '@/hooks/queries/useTransactionsList';
import { useSpendTrend } from '@/hooks/queries/useSpendTrend';
import { useDashboardCategories } from '@/hooks/queries/useDashboardCategories';
import { usePendingReviewCount } from '@/hooks/queries/usePendingReviewCount';
import { useUpcomingBills } from '@/hooks/queries/useUpcomingBills';
import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { classifyBillUrgency } from '@/components/dashboard/classifyBillUrgency';
import { computeMonthOverMonthDelta } from '@/components/dashboard/computeMonthOverMonthDelta';
import { groupCategoriesForChart } from '@/components/dashboard/groupCategoriesForChart';
import type { SpendTrendGranularity } from '@/lib/ipc';

/** Current month as YYYY-MM, the key the category query expects. */
function currentMonthString(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}`;
}

/** Loads and combines the dashboard's independent queries. */
export function useDashboardData() {
  const [granularity, setGranularity] = useState<SpendTrendGranularity>('daily');

  const { data: summary, isLoading: summaryLoading } = useDashboardSummary();
  const { data: txPage, isLoading: txLoading } = useTransactionsList(1);
  const { data: monthlyTrend } = useSpendTrend('monthly');
  const { data: trendData, isLoading: trendLoading } = useSpendTrend(granularity);
  const { data: categories, isLoading: categoriesLoading } = useDashboardCategories(
    currentMonthString()
  );
  const { data: pending } = usePendingReviewCount();
  const { data: bills } = useUpcomingBills();
  const { data: clusters = [] } = useReconciliationClusters();

  const urgentBills =
    bills?.filter((b) => {
      const u = classifyBillUrgency(b.due_date);
      return u === 'overdue' || u === 'critical';
    }) ?? [];

  const categorySlices = groupCategoriesForChart(categories);
  const hasAttentionItems =
    (pending?.count ?? 0) > 0 || urgentBills.length > 0 || clusters.length > 0;

  return {
    granularity,
    setGranularity,
    summary,
    loading: summaryLoading || txLoading,
    transactions: (txPage?.records ?? []).slice(0, 6),
    trendData,
    trendLoading,
    categorySlices,
    categoriesLoading,
    delta: computeMonthOverMonthDelta(monthlyTrend),
    pending,
    urgentBills,
    clusters,
    hasAttentionItems,
  };
}
