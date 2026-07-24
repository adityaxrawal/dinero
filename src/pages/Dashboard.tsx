import { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  TrendingUp,
  TrendingDown,
  ArrowUpRight,
  ArrowDownRight,
  AlertCircle,
  GitMerge,
  ChevronRight,
  Loader2,
  Calendar,
  AlertTriangle,
} from 'lucide-react';
import RecentTransactions from '@/components/dashboard/RecentTransactions';
import StaleClusterReminder from '@/components/dashboard/StaleClusterReminder';
import { useDashboardSummary } from '@/hooks/queries/useDashboardSummary';
import { useTransactionsList } from '@/hooks/queries/useTransactionsList';
import { useSpendTrend } from '@/hooks/queries/useSpendTrend';
import { useDashboardCategories } from '@/hooks/queries/useDashboardCategories';
import { usePendingReviewCount } from '@/hooks/queries/usePendingReviewCount';
import { useUpcomingBills } from '@/hooks/queries/useUpcomingBills';
import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { classifyBillUrgency } from '@/components/dashboard/classifyBillUrgency';
import { computeMonthOverMonthDelta } from '@/components/dashboard/computeMonthOverMonthDelta';
import {
  groupCategoriesForChart,
  type CategoryChartSlice,
} from '@/components/dashboard/groupCategoriesForChart';
import type { SpendTrendGranularity, SpendTrendPoint } from '@/lib/ipc';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
} from 'recharts';
import { SEQUENTIAL_LINE_COLOR } from '@/components/dashboard/chartPalette';

function currentMonthString(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}`;
}

function formatGreeting(): string {
  const h = new Date().getHours();
  if (h < 12) return 'Good morning';
  if (h < 18) return 'Good afternoon';
  return 'Good evening';
}

/** ── KPI Tile ─────────────────────────────────────────────── */
function KpiTile({
  label,
  value,
  valueColor,
  delta,
  deltaLabel,
  icon,
  iconBg,
}: {
  label: string;
  value: string;
  valueColor?: string;
  delta?: number | null;
  deltaLabel?: string;
  icon: React.ReactNode;
  iconBg: string;
}) {
  return (
    <div className="kpi-tile flex-1 min-w-0">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p className="kpi-label">{label}</p>
          <p className="kpi-value mt-1.5" style={{ color: valueColor ?? 'var(--text-primary)' }}>
            {value}
          </p>
          {delta != null && (
            <p className="kpi-delta" style={{ color: delta > 0 ? '#ef4444' : '#10b981' }}>
              {delta > 0 ? (
                <TrendingUp className="w-3 h-3" aria-hidden="true" />
              ) : (
                <TrendingDown className="w-3 h-3" aria-hidden="true" />
              )}
              {Math.abs(delta).toFixed(1)}% {deltaLabel ?? 'vs last month'}
            </p>
          )}
        </div>
        <div
          className="flex items-center justify-center rounded-xl flex-shrink-0"
          style={{ width: 36, height: 36, backgroundColor: iconBg }}
          aria-hidden="true"
        >
          {icon}
        </div>
      </div>
    </div>
  );
}

/** ── Monthly Limit Bar ─────────────────────────────────────── */
function LimitBar({ spent, limit }: { spent: number; limit: number }) {
  const pct = limit > 0 ? Math.min(100, (spent / limit) * 100) : 0;
  const color = pct > 90 ? '#ef4444' : pct > 75 ? '#f59e0b' : '#064E3B';
  return (
    <div className="kpi-tile flex-1 min-w-0">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="kpi-label">Monthly Limit</p>
          <p className="kpi-value mt-1.5" style={{ color: color }}>
            {pct.toFixed(0)}%
          </p>
          <p className="kpi-delta" style={{ color: 'var(--text-muted)' }}>
            ₹{spent.toLocaleString()} of ₹{limit.toLocaleString()}
          </p>
        </div>
      </div>
      <div className="mt-3">
        <div
          className="w-full h-1.5 rounded-full overflow-hidden"
          style={{ background: 'rgba(6,78,59,0.10)' }}
          role="progressbar"
          aria-valuenow={Math.round(pct)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={`${Math.round(pct)}% of monthly limit used`}
        >
          <div
            className="h-full rounded-full transition-all duration-500"
            style={{ width: `${pct}%`, backgroundColor: color }}
          />
        </div>
      </div>
    </div>
  );
}

/** ── Attention Card (scrollable rail item) ─────────────────── */
function AttentionCard({
  icon,
  iconBg,
  title,
  subtitle,
  ctaLabel,
  onClick,
  urgent,
}: {
  icon: React.ReactNode;
  iconBg: string;
  title: string;
  subtitle: string;
  ctaLabel: string;
  onClick: () => void;
  urgent?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="attention-card text-left focus-visible:outline-none"
      style={{
        borderColor: urgent ? 'rgba(245, 158, 11, 0.4)' : undefined,
        background: urgent ? 'rgba(245, 158, 11, 0.06)' : undefined,
      }}
    >
      <div
        className="flex items-center justify-center rounded-xl"
        style={{ width: 34, height: 34, backgroundColor: iconBg }}
        aria-hidden="true"
      >
        {icon}
      </div>
      <div className="min-w-0">
        <p className="text-sm font-semibold leading-tight" style={{ color: 'var(--text-primary)' }}>
          {title}
        </p>
        <p className="text-xs mt-0.5 leading-snug" style={{ color: 'var(--text-muted)' }}>
          {subtitle}
        </p>
      </div>
      <div
        className="flex items-center gap-1 text-xs font-medium mt-auto"
        style={{ color: '#064E3B' }}
      >
        {ctaLabel}
        <ChevronRight className="w-3 h-3" aria-hidden="true" />
      </div>
    </button>
  );
}

/** ── Spend Trend Chart ──────────────────────────────────────── */
function TrendChart({ data }: { data: SpendTrendPoint[] }) {
  if (!data || data.length === 0) {
    return (
      <div
        className="h-48 flex items-center justify-center text-sm"
        style={{ color: 'var(--text-muted)' }}
      >
        No spend recorded in this window yet.
      </div>
    );
  }
  return (
    <div className="h-48" role="img" aria-label="Line chart of spend over time">
      <ResponsiveContainer width="100%" height="100%">
        <LineChart data={data} margin={{ top: 4, right: 8, left: -20, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="rgba(6,78,59,0.10)" vertical={false} />
          <XAxis
            dataKey="period"
            tick={{ fontSize: 10, fill: '#6b8a7f' }}
            axisLine={false}
            tickLine={false}
          />
          <YAxis
            tick={{ fontSize: 10, fill: '#6b8a7f' }}
            axisLine={false}
            tickLine={false}
            width={52}
          />
          <Tooltip
            formatter={(value: any) => [`₹ ${Number(value).toLocaleString()}`, 'Spend']}
            contentStyle={{
              background: 'hsl(38, 55%, 91%)',
              border: '1px solid #d9c8a8',
              borderRadius: 8,
              fontSize: 12,
              color: '#0d2b22',
            }}
          />
          <Line
            type="monotone"
            dataKey="total_spend"
            stroke={SEQUENTIAL_LINE_COLOR}
            strokeWidth={2}
            dot={{ r: 3, fill: SEQUENTIAL_LINE_COLOR }}
            activeDot={{ r: 5, fill: '#064E3B' }}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}

/** ── Category Donut ────────────────────────────────────────── */
function CategoryDonut({
  slices,
  onSliceClick,
}: {
  slices: CategoryChartSlice[];
  onSliceClick: (id: string) => void;
}) {
  if (slices.length === 0) {
    return (
      <div
        className="h-full flex items-center justify-center text-sm"
        style={{ color: 'var(--text-muted)' }}
      >
        No spend yet this month.
      </div>
    );
  }
  return (
    <div style={{ height: 220 }} role="img" aria-label="Donut chart of spend by category">
      <ResponsiveContainer width="100%" height="100%">
        <PieChart>
          <Pie
            data={slices}
            dataKey="total_spend"
            nameKey="name"
            innerRadius="52%"
            outerRadius="78%"
            paddingAngle={2}
            onClick={(entry: any) => onSliceClick(entry.payload?.category_id || entry.category_id)}
            cursor="pointer"
          >
            {slices.map((slice) => (
              <Cell key={slice.category_id} fill={slice.color} />
            ))}
          </Pie>
          <Tooltip
            formatter={(value: any) => [`₹ ${Number(value).toLocaleString()}`, undefined as any]}
            contentStyle={{
              background: 'hsl(38, 55%, 91%)',
              border: '1px solid #d9c8a8',
              borderRadius: 8,
              fontSize: 12,
              color: '#0d2b22',
            }}
          />
        </PieChart>
      </ResponsiveContainer>
    </div>
  );
}

/** ── Main Dashboard ─────────────────────────────────────────── */
export default function Dashboard() {
  const navigate = useNavigate();
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

  const loading = summaryLoading || txLoading;
  const transactions = (txPage?.records ?? []).slice(0, 6);
  const delta = computeMonthOverMonthDelta(monthlyTrend);
  const categorySlices = groupCategoriesForChart(categories);

  const urgentBills =
    bills?.filter((b) => {
      const u = classifyBillUrgency(b.due_date);
      return u === 'overdue' || u === 'critical';
    }) ?? [];

  const handleCategoryClick = useCallback(
    (id: string) => {
      if (id !== '__other__') navigate(`/transactions?category=${encodeURIComponent(id)}`);
    },
    [navigate]
  );

  if (loading || !summary) {
    return (
      <div
        className="flex h-full w-full items-center justify-center"
        role="status"
        aria-label="Loading dashboard"
      >
        <Loader2 className="w-5 h-5 animate-spin" style={{ color: '#064E3B' }} aria-hidden="true" />
        <span className="sr-only">Loading dashboard…</span>
      </div>
    );
  }

  const net = summary.income - summary.month_to_date_spend;

  const hasAttentionItems =
    (pending?.count ?? 0) > 0 || urgentBills.length > 0 || clusters.length > 0;

  return (
    <div
      className="flex-1 w-full h-full overflow-y-auto animate-fade-in"
      style={{ padding: '28px 32px 40px' }}
    >
      {/* ── Header ──────────────────────────────────────────── */}
      <header className="flex items-center justify-between mb-6">
        <div>
          <h1 className="page-title" style={{ fontSize: '22px', fontWeight: 700 }}>
            {formatGreeting()}
          </h1>
          <p className="text-sm mt-0.5" style={{ color: 'var(--text-muted)' }}>
            {new Date().toLocaleDateString(undefined, {
              weekday: 'long',
              month: 'long',
              day: 'numeric',
            })}
          </p>
        </div>
        <span
          className="text-xs font-medium px-3 py-1 rounded-full"
          style={{
            background: 'rgba(6,78,59,0.08)',
            color: '#064E3B',
            border: '1px solid rgba(6,78,59,0.15)',
          }}
        >
          {new Date().toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}
        </span>
      </header>

      {/* ── KPI Row ─────────────────────────────────────────── */}
      <section aria-label="Key metrics" className="flex gap-3 mb-5">
        <KpiTile
          label="Total Spend"
          value={`₹${summary.month_to_date_spend.toLocaleString()}`}
          valueColor="#ef4444"
          delta={delta}
          icon={<ArrowUpRight className="w-4 h-4" style={{ color: '#ef4444' }} />}
          iconBg="rgba(239, 68, 68, 0.10)"
        />
        <KpiTile
          label="Income"
          value={`₹${summary.income.toLocaleString()}`}
          valueColor="#10b981"
          icon={<ArrowDownRight className="w-4 h-4" style={{ color: '#10b981' }} />}
          iconBg="rgba(16, 185, 129, 0.10)"
        />
        <KpiTile
          label="Net"
          value={`${net >= 0 ? '+' : ''}₹${Math.abs(net).toLocaleString()}`}
          valueColor={net >= 0 ? '#10b981' : '#ef4444'}
          icon={
            net >= 0 ? (
              <ArrowDownRight className="w-4 h-4" style={{ color: '#10b981' }} />
            ) : (
              <ArrowUpRight className="w-4 h-4" style={{ color: '#ef4444' }} />
            )
          }
          iconBg={net >= 0 ? 'rgba(16, 185, 129, 0.10)' : 'rgba(239, 68, 68, 0.10)'}
        />
        {summary.limit > 0 && (
          <LimitBar spent={summary.month_to_date_spend} limit={summary.limit} />
        )}
      </section>

      {/* ── Attention Rail ───────────────────────────────────── */}
      {hasAttentionItems && (
        <section aria-label="Items needing attention" className="mb-5">
          <p className="section-heading" style={{ paddingLeft: 0 }}>
            Needs Attention
          </p>
          <div className="attention-rail mt-2">
            {(pending?.count ?? 0) > 0 && (
              <AttentionCard
                icon={<AlertCircle className="w-4 h-4" style={{ color: '#f59e0b' }} />}
                iconBg="rgba(245, 158, 11, 0.12)"
                title={`${pending!.count} Pending Review`}
                subtitle={`₹${(pending!.amount_minor / 100).toLocaleString(undefined, { minimumFractionDigits: 0 })} not yet confirmed`}
                ctaLabel="Review"
                onClick={() => navigate('/reconciliation')}
                urgent
              />
            )}
            {urgentBills.map((bill) => {
              const u = classifyBillUrgency(bill.due_date);
              return (
                <AttentionCard
                  key={bill.id}
                  icon={
                    <AlertTriangle
                      className="w-4 h-4"
                      style={{ color: u === 'overdue' ? '#ef4444' : '#f59e0b' }}
                    />
                  }
                  iconBg={u === 'overdue' ? 'rgba(239,68,68,0.12)' : 'rgba(245,158,11,0.12)'}
                  title={bill.description}
                  subtitle={`₹${bill.amount.toLocaleString()} · Due ${new Date(bill.due_date).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}`}
                  ctaLabel={u === 'overdue' ? 'Overdue' : 'Due soon'}
                  onClick={() => navigate('/instruments')}
                  urgent={u === 'overdue'}
                />
              );
            })}
            {clusters.length > 0 && (
              <AttentionCard
                icon={<GitMerge className="w-4 h-4" style={{ color: '#064E3B' }} />}
                iconBg="rgba(6,78,59,0.10)"
                title={`${clusters.length} Unresolved Cluster${clusters.length > 1 ? 's' : ''}`}
                subtitle="Ambiguous transaction matches"
                ctaLabel="Resolve"
                onClick={() => navigate('/reconciliation')}
              />
            )}
            {summary.upcoming_bills_count > 0 && urgentBills.length === 0 && (
              <AttentionCard
                icon={<Calendar className="w-4 h-4" style={{ color: '#3d5a50' }} />}
                iconBg="rgba(6,78,59,0.08)"
                title={`${summary.upcoming_bills_count} Upcoming Bill${summary.upcoming_bills_count > 1 ? 's' : ''}`}
                subtitle="Statement due dates"
                ctaLabel="View"
                onClick={() => navigate('/instruments')}
              />
            )}
          </div>
        </section>
      )}

      {/* ── Charts Row ───────────────────────────────────────── */}
      <section
        aria-label="Spending analytics"
        className="grid gap-4 mb-5"
        style={{ gridTemplateColumns: '1fr 340px' }}
      >
        {/* Trend Chart */}
        <div className="card-champagne p-5">
          <div className="flex items-center justify-between mb-4">
            <div>
              <h2 className="heading-sm">Spend Trend</h2>
              <p className="text-xs mt-0.5" style={{ color: 'var(--text-muted)' }}>
                Confirmed spend over time
              </p>
            </div>
            <div
              className="flex gap-0.5 p-0.5 rounded-lg"
              style={{ background: 'rgba(6,78,59,0.06)', border: '1px solid rgba(6,78,59,0.10)' }}
              role="group"
              aria-label="Granularity"
            >
              {(['daily', 'weekly', 'monthly'] as SpendTrendGranularity[]).map((g) => (
                <button
                  key={g}
                  type="button"
                  onClick={() => setGranularity(g)}
                  className="text-xs font-medium px-2.5 py-1 rounded-md transition-all"
                  style={{
                    background: granularity === g ? '#064E3B' : 'transparent',
                    color: granularity === g ? '#F8E7C9' : '#6b8a7f',
                  }}
                  aria-pressed={granularity === g}
                >
                  {g.charAt(0).toUpperCase() + g.slice(1, 3)}
                </button>
              ))}
            </div>
          </div>
          {trendLoading ? (
            <div className="h-48 flex items-center justify-center">
              <Loader2 className="w-4 h-4 animate-spin" style={{ color: '#064E3B' }} />
            </div>
          ) : (
            <TrendChart data={trendData ?? []} />
          )}
        </div>

        {/* Category Donut */}
        <div className="card-champagne p-5">
          <h2 className="heading-sm mb-0.5">By Category</h2>
          <p className="text-xs mb-3" style={{ color: 'var(--text-muted)' }}>
            This month's spend
          </p>
          {categoriesLoading ? (
            <div className="h-[220px] flex items-center justify-center">
              <Loader2 className="w-4 h-4 animate-spin" style={{ color: '#064E3B' }} />
            </div>
          ) : (
            <CategoryDonut slices={categorySlices} onSliceClick={handleCategoryClick} />
          )}
          {/* Legend */}
          {categorySlices.length > 0 && (
            <ul className="mt-2 space-y-1">
              {categorySlices.slice(0, 5).map((s) => (
                <li key={s.category_id} className="flex items-center gap-2">
                  <span
                    className="w-2.5 h-2.5 rounded-sm flex-shrink-0"
                    style={{ background: s.color }}
                    aria-hidden="true"
                  />
                  <button
                    type="button"
                    className="text-xs truncate hover:underline text-left"
                    style={{ color: 'var(--text-secondary)' }}
                    onClick={() => handleCategoryClick(s.category_id)}
                    disabled={s.category_id === '__other__'}
                  >
                    {s.name}
                  </button>
                  <span
                    className="ml-auto text-xs font-medium amount"
                    style={{ color: 'var(--text-primary)' }}
                  >
                    ₹{s.total_spend.toLocaleString()}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>

      <StaleClusterReminder clusters={clusters} />

      {/* ── Recent Transactions ──────────────────────────────── */}
      <RecentTransactions transactions={transactions} />
    </div>
  );
}
