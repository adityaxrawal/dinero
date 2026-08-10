/**
 * The dashboard: headline figures, spend trends, and items needing attention.
 *
 * The landing screen, so it composes several independent queries rather than one
 * aggregate -- each panel loads and refreshes on its own and a slow one cannot
 * hold up the rest.
 */
import { useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Loader2 } from 'lucide-react';
import RecentTransactions from '@/components/dashboard/RecentTransactions';
import StaleClusterReminder from '@/components/dashboard/StaleClusterReminder';
import { useDashboardData } from './dashboard/useDashboardData';
import KpiRow from './dashboard/KpiRow';
import AttentionRail from './dashboard/AttentionRail';
import ChartsRow from './dashboard/ChartsRow';

/** Time-of-day greeting for the dashboard header. */
function formatGreeting(): string {
  const h = new Date().getHours();
  if (h < 12) return 'Good morning';
  if (h < 18) return 'Good afternoon';
  return 'Good evening';
}

/** Greeting, date range and last-synced indicator. */
function DashboardHeader() {
  const now = new Date();
  return (
    <header className="flex items-center justify-between mb-6">
      <div>
        <h1 className="page-title" style={{ fontSize: '22px', fontWeight: 700 }}>
          {formatGreeting()}
        </h1>
        <p className="text-sm mt-0.5" style={{ color: 'var(--text-muted)' }}>
          {now.toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' })}
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
        {now.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}
      </span>
    </header>
  );
}

/** The dashboard: KPIs, charts, and items needing attention. */
export default function Dashboard() {
  const navigate = useNavigate();
  const data = useDashboardData();
  const { summary } = data;

  const handleCategoryClick = useCallback(
    (id: string) => {
      if (id !== '__other__') navigate(`/transactions?category=${encodeURIComponent(id)}`);
    },
    [navigate]
  );

  if (data.loading || !summary) {
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

  return (
    <div
      className="flex-1 w-full h-full overflow-y-auto animate-fade-in"
      style={{ padding: '28px 32px 40px' }}
    >
      <DashboardHeader />

      <KpiRow
        spend={summary.month_to_date_spend}
        income={summary.income}
        limit={summary.limit}
        delta={data.delta}
      />

      {data.hasAttentionItems && <AttentionRail data={data} />}

      <ChartsRow data={data} onCategoryClick={handleCategoryClick} />

      <StaleClusterReminder clusters={data.clusters} />

      <RecentTransactions transactions={data.transactions} />
    </div>
  );
}
