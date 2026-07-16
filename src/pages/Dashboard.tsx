import { useState, useEffect, useRef } from 'react';
import { Activity, CreditCard, ChevronDown, ChevronRight, CheckCircle2, PlusCircle } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useNavigate } from 'react-router-dom';
import { useDashboardSummary } from '@/hooks/queries/useDashboardSummary';
import { useTransactionsList } from '@/hooks/queries/useTransactionsList';
import { useInstrumentsList } from '@/hooks/queries/useInstrumentsList';
import { useSpendTrend } from '@/hooks/queries/useSpendTrend';
import { useDashboardCategories } from '@/hooks/queries/useDashboardCategories';
import type { InstrumentRecord } from '@/lib/ipc';
import SpendSummaryCard from '@/components/dashboard/SpendSummaryCard';
import CategoryBreakdownChart from '@/components/dashboard/CategoryBreakdownChart';
import SpendTrendChart from '@/components/dashboard/SpendTrendChart';
import UpcomingBillsWidget from '@/components/dashboard/UpcomingBillsWidget';
import PendingReviewBanner from '@/components/dashboard/PendingReviewBanner';

function currentMonthString(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}`;
}

export default function Dashboard() {
  const navigate = useNavigate();
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});

  // TASK-FE-003: React Query hooks replace the old manual useState/useEffect
  // fetch + hand-rolled transaction_created/scan_completed listeners — the
  // globally-mounted useIpcQueryInvalidation (App.tsx's IpcEventBridge)
  // already invalidates these exact query keys on those real events, so a
  // second, page-local subscription would just be duplicate work.
  const { data: summary, isLoading: summaryLoading } = useDashboardSummary();
  const { data: txPage, isLoading: txLoading } = useTransactionsList(1);
  const { data: instruments = [], isLoading: instrumentsLoading } = useInstrumentsList();
  const { data: monthlyTrend } = useSpendTrend('monthly');
  const { data: categories, isLoading: categoriesLoading } = useDashboardCategories(currentMonthString());

  const toggleGroup = (type: string) => {
    setExpandedGroups((prev) => ({ ...prev, [type]: !prev[type] }));
  };

  // Auto-expand the first instrument group once, same as the pre-React-Query
  // version — but only the first time instruments load, not on every
  // background refetch (which would clobber a user's manual toggles).
  const hasAutoExpanded = useRef(false);
  useEffect(() => {
    if (!hasAutoExpanded.current && instruments.length > 0) {
      hasAutoExpanded.current = true;
      setExpandedGroups({ [instruments[0].instrument_type]: true });
    }
  }, [instruments]);

  const loading = summaryLoading || txLoading || instrumentsLoading;

  if (loading || !summary) {
    return (
      <div className="flex h-full w-full items-center justify-center" role="status" aria-label="Loading dashboard">
        <Activity className="w-6 h-6 animate-spin text-muted-foreground" aria-hidden="true" />
        <span className="sr-only">Loading dashboard…</span>
      </div>
    );
  }

  const transactions = (txPage?.records ?? []).slice(0, 5);
  const limitPercentage = summary.limit > 0
    ? Math.min((summary.month_to_date_spend / summary.limit) * 100, 100)
    : 0;

  const groupedInstruments = instruments.reduce((acc, inst) => {
    const key = inst.instrument_type;
    if (!acc[key]) acc[key] = [];
    acc[key].push(inst);
    return acc;
  }, {} as Record<string, InstrumentRecord[]>);

  return (
    <div className="space-y-8 animate-in fade-in duration-500">
      <header>
        <h1 className="text-3xl font-bold tracking-tight">Overview</h1>
        <p className="text-muted-foreground mt-1">Your financial summary for this month.</p>
      </header>

      <PendingReviewBanner />

      <section aria-label="Key performance indicators">
        <SpendSummaryCard summary={summary} monthlyTrend={monthlyTrend} />
      </section>

      <div className="grid gap-6 grid-cols-1 lg:grid-cols-2">
        <CategoryBreakdownChart categories={categories} isLoading={categoriesLoading} />
        <SpendTrendChart />
      </div>

      <div className="grid gap-6 grid-cols-1 lg:grid-cols-3">
        {/* Main Feed */}
        <div className="lg:col-span-2 space-y-6">
          <Card>
            <CardHeader>
              <CardTitle>Recent Transactions</CardTitle>
              <CardDescription>Your most recent canonical transaction records.</CardDescription>
            </CardHeader>
            <CardContent>
              {transactions.length === 0 ? (
                <div className="text-center py-6 text-muted-foreground flex flex-col items-center" role="status">
                  <CheckCircle2 className="w-10 h-10 mb-2 opacity-20" aria-hidden="true" />
                  <p>No transactions exist. Sync your bank or upload a statement to get started.</p>
                </div>
              ) : (
                <div className="space-y-4" aria-label="Recent transactions">
                  {transactions.map((tx) => (
                    <div
                      key={tx.id}
                      className="flex items-center justify-between p-3 rounded-lg hover:bg-secondary/50 transition-colors border border-transparent hover:border-border cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                      onClick={() => navigate('/transactions')}
                      onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && navigate('/transactions')}
                      tabIndex={0}
                      aria-label={`${tx.merchant}, ${tx.amount < 0 ? 'spent' : 'received'} ₹${Math.abs(tx.amount).toLocaleString()}`}
                    >
                      <div className="flex items-center gap-4">
                        <div className="w-10 h-10 rounded-full bg-primary/10 flex items-center justify-center text-primary font-bold text-sm" aria-hidden="true">
                          {tx.merchant.charAt(0)}
                        </div>
                        <div>
                          <p className="font-medium">{tx.merchant}</p>
                          <p className="text-xs text-muted-foreground">{new Date(tx.date).toLocaleString()} • {tx.category}</p>
                        </div>
                      </div>
                      <div className="text-right">
                        <p className={cn('font-medium', tx.amount < 0 ? 'text-red-700' : 'text-emerald-700')}>
                          {tx.amount < 0 ? '- ' : '+ '}₹{Math.abs(tx.amount).toLocaleString(undefined, { minimumFractionDigits: 2 })}
                        </p>
                        <Badge variant={tx.status.toLowerCase() === 'posted' ? 'default' : 'secondary'} className="mt-1 text-[10px] px-1.5 py-0">
                          {tx.status}
                        </Badge>
                      </div>
                    </div>
                  ))}
                  <Button variant="outline" className="w-full mt-2" onClick={() => navigate('/transactions')} aria-label="View all transactions">
                    View All Transactions
                  </Button>
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* Sidebar / Instruments */}
        <div className="space-y-6">
          <UpcomingBillsWidget />

          <Card aria-label={`Monthly limit: spent ₹${summary.month_to_date_spend.toLocaleString()} of ₹${summary.limit.toLocaleString()}`}>
            <CardHeader>
              <CardTitle>Monthly Limit</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="flex justify-between text-sm mb-2">
                <span className="text-muted-foreground">Spent</span>
                <span className="font-medium">₹ {summary.month_to_date_spend.toLocaleString()} / ₹ {summary.limit.toLocaleString()}</span>
              </div>
              <div
                className="w-full h-2.5 bg-secondary rounded-full overflow-hidden"
                role="progressbar"
                aria-valuenow={Math.round(limitPercentage)}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-label={`${Math.round(limitPercentage)}% of monthly limit used`}
              >
                <div
                  className={cn('h-full rounded-full', limitPercentage > 90 ? 'bg-destructive' : 'bg-primary')}
                  style={{ width: `${limitPercentage}%` }}
                />
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Accounts &amp; Cards</CardTitle>
              <CardDescription>Your linked accounts &amp; cards.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4 px-2">
              {Object.keys(groupedInstruments).length === 0 ? (
                <div className="text-center py-6 text-muted-foreground flex flex-col items-center gap-2">
                  <PlusCircle className="w-8 h-8 opacity-20" aria-hidden="true" />
                  <p className="text-sm">No instruments linked.</p>
                  <Button variant="link" onClick={() => navigate('/instruments')} aria-label="Add an instrument">
                    Add one now
                  </Button>
                </div>
              ) : (
                Object.entries(groupedInstruments).map(([type, insts]) => (
                  <div key={type} className="border border-border rounded-lg overflow-hidden">
                    <button
                      type="button"
                      className="w-full flex items-center justify-between p-3 bg-secondary/50 cursor-pointer hover:bg-secondary transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
                      onClick={() => toggleGroup(type)}
                      aria-expanded={!!expandedGroups[type]}
                      aria-controls={`instrument-group-${type}`}
                      aria-label={`${type.replace('_', ' ').toLowerCase()} group, ${insts.length} items`}
                    >
                      <div className="flex items-center gap-2">
                        <CreditCard className="w-4 h-4 text-muted-foreground" aria-hidden="true" />
                        <span className="text-sm font-semibold capitalize">{type.replace(/_/g, ' ').toLowerCase()}</span>
                        <Badge variant="secondary" className="ml-1 rounded-full px-2 py-0" aria-hidden="true">{insts.length}</Badge>
                      </div>
                      {expandedGroups[type]
                        ? <ChevronDown className="w-4 h-4 text-muted-foreground" aria-hidden="true" />
                        : <ChevronRight className="w-4 h-4 text-muted-foreground" aria-hidden="true" />}
                    </button>
                    {expandedGroups[type] && (
                      <div id={`instrument-group-${type}`} className="p-3 bg-card space-y-3">
                        {insts.map((inst) => (
                          <div key={inst.id} className="space-y-1.5">
                            <div className="flex justify-between items-center">
                              <div>
                                <p className="text-sm font-medium">{inst.issuer_name}</p>
                                <p className="text-xs text-muted-foreground">{inst.masked_identifier}</p>
                              </div>
                              <Badge variant={inst.status === 'active' ? 'default' : 'secondary'} className="text-[10px]">
                                {inst.status}
                              </Badge>
                            </div>
                            {inst.instrument_type === 'credit_card' && (
                              <div className="mt-2 text-xs">
                                <div className="flex justify-between text-muted-foreground mb-1">
                                  <span>Utilization</span>
                                  <span>
                                    {inst.credit_limit ? (
                                       `${Math.min(100, Math.max(0, ((inst.current_balance || 0) / inst.credit_limit) * 100)).toFixed(1)}%`
                                    ) : (
                                       'N/A'
                                    )}
                                  </span>
                                </div>
                                <div className="w-full h-1 bg-secondary rounded-full overflow-hidden">
                                  <div
                                    className={cn('h-full rounded-full', inst.credit_limit && ((inst.current_balance || 0) / inst.credit_limit) > 0.9 ? 'bg-destructive' : 'bg-primary')}
                                    style={{ width: `${inst.credit_limit ? Math.min(100, Math.max(0, ((inst.current_balance || 0) / inst.credit_limit) * 100)) : 0}%` }}
                                  />
                                </div>
                              </div>
                            )}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                ))
              )}
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
