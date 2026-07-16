import { useEffect, useState, useCallback } from 'react';
import { ArrowUpRight, ArrowDownRight, Activity, CreditCard, ChevronDown, ChevronRight, CheckCircle2, PlusCircle } from 'lucide-react';
import { API, DashboardSummary, TransactionRecord, InstrumentRecord } from '../lib/ipc';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useNavigate } from 'react-router-dom';


// Tauri event listener — no-op in browser mode
let tauriListen: ((event: string, handler: (e: any) => void) => Promise<() => void>) | null = null;
try {
  import('@tauri-apps/api/event').then((m) => {
    tauriListen = m.listen;
  });
} catch {
  // Running in browser dev mode
}

export default function Dashboard() {
  const navigate = useNavigate();
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [transactions, setTransactions] = useState<TransactionRecord[]>([]);
  const [instruments, setInstruments] = useState<InstrumentRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});

  const toggleGroup = (type: string) => {
    setExpandedGroups((prev) => ({ ...prev, [type]: !prev[type] }));
  };

  const fetchData = useCallback(async () => {
    try {
      const [sum, txs, insts] = await Promise.all([
        API.dashboard.getSummary(),
        API.transactions.list(),
        API.instruments.list(),
      ]);
      setSummary(sum);
      setTransactions(txs.records.slice(0, 5));
      setInstruments(insts);
      // Auto-expand first group
      if (insts.length > 0) {
        setExpandedGroups({ [insts[0].instrument_type]: true });
      }
    } catch (err) {
      console.error('Failed to fetch dashboard data:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();

    if (!tauriListen) {
      // Browser mode — no live event subscriptions
      return;
    }

    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      if (!tauriListen) return;

      // Subscribe to transaction.created for live dashboard updates
      const unlistenTx = await tauriListen('transaction_created', () => {
        fetchData();
      });
      unlisteners.push(unlistenTx);

      // Subscribe to scan.completed for live dashboard updates
      const unlistenScan = await tauriListen('scan_completed', () => {
        fetchData();
      });
      unlisteners.push(unlistenScan);

      // Subscribe to statement.parsed for live dashboard updates
      const unlistenStmt = await tauriListen('statement_parsed', () => {
        fetchData();
      });
      unlisteners.push(unlistenStmt);
    };

    setup().catch(console.error);

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [fetchData]);

  if (loading || !summary) {
    return (
      <div className="flex h-full w-full items-center justify-center" role="status" aria-label="Loading dashboard">
        <Activity className="w-6 h-6 animate-spin text-muted-foreground" aria-hidden="true" />
        <span className="sr-only">Loading dashboard…</span>
      </div>
    );
  }

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

      {/* No hardcoded subscription alert — only show if real data exists */}

      {/* KPI Cards */}
      <section aria-label="Key performance indicators">
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

          <Card aria-label={`Upcoming bills: ${summary.upcoming_bills_count} pending`}>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm font-medium">Upcoming Bills</CardTitle>
              <div className="p-2 bg-amber-500/10 rounded-md" aria-hidden="true">
                <Activity className="h-4 w-4 text-amber-500" />
              </div>
            </CardHeader>
            <CardContent>
              <div className="text-2xl font-bold">{summary.upcoming_bills_count} Pending</div>
            </CardContent>
          </Card>
        </div>
      </section>

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
                  <p>No transactions found.</p>
                  <Button variant="link" onClick={() => navigate('/statements')} aria-label="Upload a statement to get started">
                    Upload a statement
                  </Button>
                </div>
              ) : (
                <div className="space-y-4"  aria-label="Recent transactions">
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
