import { ArrowUpRight, ArrowDownRight, TrendingUp, TrendingDown } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { cn } from '@/lib/utils';
import type { DashboardSummary, SpendTrendPoint } from '@/lib/ipc';
import { computeMonthOverMonthDelta } from './computeMonthOverMonthDelta';

interface SpendSummaryCardProps {
  summary: DashboardSummary;
  monthlyTrend: SpendTrendPoint[] | undefined;
}

/**
 * TASK-FE-008 (Doc 30): total spend/income/net, month-over-month delta.
 * `DashboardSummary` itself carries no delta field — derived from the
 * monthly spend-trend series instead (see `computeMonthOverMonthDelta`).
 */
export default function SpendSummaryCard({ summary, monthlyTrend }: SpendSummaryCardProps) {
  const net = summary.income - summary.month_to_date_spend;
  const delta = computeMonthOverMonthDelta(monthlyTrend);

  return (
    <div className="grid gap-6 grid-cols-1 md:grid-cols-3">
      <Card aria-label={`Total spend this month: ₹${summary.month_to_date_spend.toLocaleString()}`}>
        <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
          <CardTitle className="text-sm font-medium">Total Spend (MTD)</CardTitle>
          <div className="p-2 bg-destructive/10 rounded-md" aria-hidden="true">
            <ArrowUpRight className="h-4 w-4 text-red-700" />
          </div>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold">₹ {summary.month_to_date_spend.toLocaleString()}</div>
          {delta !== null && (
            <p
              className={cn(
                'flex items-center gap-1 text-xs mt-1',
                delta > 0 ? 'text-red-700' : 'text-emerald-700',
              )}
            >
              {delta > 0 ? <TrendingUp className="w-3 h-3" aria-hidden="true" /> : <TrendingDown className="w-3 h-3" aria-hidden="true" />}
              {Math.abs(delta).toFixed(1)}% vs last month
            </p>
          )}
        </CardContent>
      </Card>

      <Card aria-label={`Income this month: ₹${summary.income.toLocaleString()}`}>
        <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
          <CardTitle className="text-sm font-medium">Income (MTD)</CardTitle>
          <div className="p-2 bg-emerald-500/10 rounded-md" aria-hidden="true">
            <ArrowDownRight className="h-4 w-4 text-emerald-700" />
          </div>
        </CardHeader>
        <CardContent>
          <div className="text-2xl font-bold">₹ {summary.income.toLocaleString()}</div>
        </CardContent>
      </Card>

      <Card aria-label={`Net this month: ₹${net.toLocaleString()}`}>
        <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
          <CardTitle className="text-sm font-medium">Net (MTD)</CardTitle>
          <div className={cn('p-2 rounded-md', net >= 0 ? 'bg-emerald-500/10' : 'bg-destructive/10')} aria-hidden="true">
            {net >= 0 ? <ArrowDownRight className="h-4 w-4 text-emerald-700" /> : <ArrowUpRight className="h-4 w-4 text-red-700" />}
          </div>
        </CardHeader>
        <CardContent>
          <div className={cn('text-2xl font-bold', net >= 0 ? 'text-emerald-700' : 'text-red-700')}>
            {net >= 0 ? '+' : '-'}₹ {Math.abs(net).toLocaleString()}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
