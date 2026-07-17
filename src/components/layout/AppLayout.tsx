import { useState, useEffect, useCallback } from 'react';
import { NavLink, Outlet, useNavigate } from 'react-router-dom';
import {
  LayoutDashboard,
  ArrowLeftRight,
  CreditCard,
  FileText,
  GitMerge,
  Settings,
  Activity,
  WifiOff,
  AlertTriangle,
  Loader2,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { cn } from '@/lib/utils';
import { API } from '@/lib/ipc';
import { useLicenseStore } from '@/stores/useLicenseStore';
import LicenseLockOverlay from '@/components/licensing/LicenseLockOverlay';
import GracePeriodBanner from '@/components/licensing/GracePeriodBanner';
import StatementOnlyModeBanner from '@/components/shell/StatementOnlyModeBanner';

// HMR trigger 3
const CORRUPTION_EVENT = 'db_corrupted';
const TASK_STARTED_EVENT = 'task_started';
const TASK_COMPLETED_EVENT = 'task_completed';

interface BackgroundTask {
  id: string;
  message: string;
}

export default function AppLayout() {
  const navigate = useNavigate();
  const [backendStatus, setBackendStatus] = useState<'healthy' | 'offline' | 'corrupted'>('healthy');
  const hydrateLicenseStore = useLicenseStore((s) => s.hydrate);
  const [bgTasks, setBgTasks] = useState<BackgroundTask[]>([]);
  const [unresolvedClusters, setUnresolvedClusters] = useState(0);
  const [isRestoring, setIsRestoring] = useState(false);

  const handleRestoreBackup = useCallback(async () => {
    setIsRestoring(true);
    try {
      await API.db.restoreBackup();
      setBackendStatus('healthy');
    } catch (err) {
      console.error('Restore backup failed:', err);
    } finally {
      setIsRestoring(false);
    }
  }, []);

  // G5 fix: corrupted-DB recovery previously offered only "Restore from
  // Backup" — "Start Fresh" gives a real alternative when backups aren't
  // trusted or wanted, instead of being stuck with one path.
  const [isStartingFresh, setIsStartingFresh] = useState(false);
  const handleStartFresh = useCallback(async () => {
    let confirmed = false;
    const warning = 'Start fresh? This permanently deletes all local data (transactions, statements, settings) and returns you to onboarding. This cannot be undone.';
    try {
      const { ask } = await import('@tauri-apps/plugin-dialog');
      confirmed = await ask(warning, { title: 'Start Fresh', kind: 'warning' });
    } catch {
      confirmed = confirm(warning);
    }
    if (!confirmed) return;

    setIsStartingFresh(true);
    try {
      await API.dev.resetDatabase();
      window.location.reload();
    } catch (err) {
      console.error('Start fresh failed:', err);
      setIsStartingFresh(false);
    }
  }, []);

  useEffect(() => {
    // Onboarding gate: only enforce inside the Tauri runtime.
    // In browser/E2E/dev mode the flag is absent from window, so we skip it to
    // avoid blocking Playwright tests that navigate directly to app routes.
    const isTauriRuntime = !!(window as any).__TAURI_INTERNALS__;
    if (isTauriRuntime && !localStorage.getItem('dinero_onboarded')) {
      navigate('/onboarding', { replace: true });
      return;
    }

    // Initial status check
    API.status.check()
      .then((result) => setBackendStatus((prev) => prev === 'healthy' || prev === 'offline' ? (result.status as 'healthy') : prev))
      .catch(() => setBackendStatus((prev) => prev === 'healthy' || prev === 'offline' ? 'offline' : prev));

    // TASK-FE-016: hydrate the license store immediately at launch so
    // LicenseLockOverlay/GracePeriodBanner reflect the real current state
    // right away, rather than waiting for the next license_state_changed
    // broadcast (which could be up to 6 hours away, the background
    // validation worker's tick interval).
    hydrateLicenseStore();

    // Load unresolved cluster count for nav badge
    API.reconciliation.listUnresolved()
      .then((clusters) => setUnresolvedClusters(clusters.length))
      .catch(() => setUnresolvedClusters(0));

    // Removed early return to allow browser mock handlers to run

    const unlisteners: (() => void)[] = [];

    const setup = async () => {

      let listen;
      try {
        const m = await import('@tauri-apps/api/event');
        listen = m.listen;
      } catch (e) {
        // Not in Tauri environment
        return;
      }

      if (!listen) return;

      // DB corruption event
      const unlistenCorrupt = await listen(CORRUPTION_EVENT, () => {
        setBackendStatus('corrupted');
      });
      unlisteners.push(unlistenCorrupt);

      // Background task started
      const unlistenTaskStarted = await listen(TASK_STARTED_EVENT, (event: { payload: BackgroundTask }) => {
        setBgTasks((prev) => {
          if (prev.some((t) => t.id === event.payload.id)) return prev;
          return [...prev, event.payload];
        });
      });
      unlisteners.push(unlistenTaskStarted);

      // Background task completed
      const unlistenTaskCompleted = await listen(TASK_COMPLETED_EVENT, (event: { payload: { id: string } }) => {
        setBgTasks((prev) => prev.filter((t) => t.id !== event.payload.id));
      });
      unlisteners.push(unlistenTaskCompleted);
      
      const unlistenAlert = await listen('alert_threshold_crossed', (event: { payload: any }) => {
        setBgTasks((prev) => [...prev, { id: 'alert_' + Date.now(), message: `Alert: ${event.payload.category} exceeded ${event.payload.threshold}% of budget` }]);
        setTimeout(() => {
          setBgTasks((prev) => prev.filter(t => !t.id.startsWith('alert_')));
        }, 5000);
      });
      unlisteners.push(unlistenAlert);
    };

    setup().catch(console.error);

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [navigate, hydrateLicenseStore]);

  const navItems = [
    { path: '/', label: 'Dashboard', icon: <LayoutDashboard size={20} />, badge: 0 },
    { path: '/transactions', label: 'Transactions', icon: <ArrowLeftRight size={20} />, badge: 0 },
    { path: '/instruments', label: 'Instruments', icon: <CreditCard size={20} />, badge: 0 },
    { path: '/statements', label: 'Statements', icon: <FileText size={20} />, badge: 0 },
    {
      path: '/reconciliation',
      label: 'Reconciliation',
      icon: <GitMerge size={20} />,
      badge: unresolvedClusters,
    },
    // G16 fix: moved into a Settings subsection rather than a top-level nav
    // item — the route ('/spending-limits') still exists and is linked from
    // Settings, it's just no longer competing for sidebar space.
    { path: '/settings', label: 'Settings', icon: <Settings size={20} />, badge: 0 },
    // F14 fix: Debug Console must not be reachable in production builds.
    ...(import.meta.env.DEV
      ? [{ path: '/debug', label: 'Debug', icon: <Activity size={20} />, badge: 0 }]
      : []),
  ];

  if (backendStatus === 'corrupted') {
    return (
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="db-corrupted-title"
        aria-describedby="db-corrupted-desc"
        className="flex flex-col items-center justify-center h-screen bg-background text-foreground p-8"
      >
        <div className="max-w-md w-full bg-card border border-border rounded-lg p-6 flex flex-col items-center text-center space-y-4">
           <div className="h-12 w-12 rounded-full bg-destructive/20 flex items-center justify-center">
             <AlertTriangle className="text-red-700 w-6 h-6" aria-hidden="true" />
           </div>
           <h2 id="db-corrupted-title" className="text-xl font-semibold">Database Corrupted</h2>
           <p id="db-corrupted-desc" className="text-sm text-muted-foreground">
             <span>The SQLite integrity check failed.</span> Restore from a backup to recover your data, or start fresh.
           </p>
           <div className="flex flex-col gap-2 w-full">
             <Button
               variant="default"
               onClick={handleRestoreBackup}
               disabled={isRestoring || isStartingFresh}
               aria-label="Restore database from backup"
             >
               {isRestoring ? (
                 <><Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Restoring...</>
               ) : (
                 'Restore from Backup'
               )}
             </Button>
             <Button
               variant="outline"
               onClick={handleStartFresh}
               disabled={isRestoring || isStartingFresh}
               aria-label="Delete all local data and start fresh"
             >
               {isStartingFresh ? (
                 <><Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Starting Fresh...</>
               ) : (
                 'Start Fresh (Delete All Data)'
               )}
             </Button>
           </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-screen w-screen bg-background text-foreground overflow-hidden">
      {/* Skip to main content link — keyboard accessibility */}
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-4 focus:z-50 focus:px-4 focus:py-2 focus:bg-background focus:border focus:border-border focus:rounded-md focus:text-sm focus:font-medium focus:shadow-lg"
      >
        Skip to main content
      </a>

      {/* Sidebar */}
      <aside
        className="w-64 bg-card border-r border-border flex flex-col p-6 z-10"
        aria-label="Main navigation"
      >
        <div className="mb-10 px-3 flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-[#2563eb] flex items-center justify-center">
            <Activity className="text-white" size={18} aria-hidden="true" />
          </div>
          <h1 className="text-xl font-semibold tracking-tight">Dinero</h1>
        </div>

        <nav className="flex-1 flex flex-col gap-1" aria-label="Primary navigation">
          {navItems.map((item) => (
            <NavLink
              key={item.path}
              to={item.path}
              end={item.path === '/'}
              aria-label={item.badge > 0 ? `${item.label} — ${item.badge} pending` : item.label}
              className={({ isActive }) =>
                cn(
                  // Base
                  'relative flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all duration-200 ease-out outline-none',
                  'focus-visible:ring-2 focus-visible:ring-[#2563eb]/60 focus-visible:ring-offset-2 focus-visible:ring-offset-background',
                  isActive
                    ? [
                        'bg-[#2563eb]/8 text-[#2563eb] font-semibold',
                        'border border-[#2563eb]/20',
                      ].join(' ')
                    : 'text-muted-foreground border border-transparent hover:bg-accent hover:text-foreground hover:translate-x-0.5'
                )
              }
            >
              {({ isActive }) => (
                <>
                  {/* Left accent bar */}
                  <span
                    aria-hidden="true"
                    className={cn(
                      'absolute left-0 top-[20%] h-[60%] w-[3px] rounded-r-full bg-[#2563eb] transition-all duration-200',
                      isActive ? 'opacity-100 scale-y-100' : 'opacity-0 scale-y-50'
                    )}
                  />
                  {/* Icon */}
                  <span
                    className={cn(
                      'shrink-0 transition-colors duration-200',
                      isActive ? 'text-[#2563eb]' : 'text-muted-foreground/60 group-hover:text-muted-foreground'
                    )}
                    aria-hidden="true"
                  >
                    {item.icon}
                  </span>
                  {item.label}
                  <div className="flex-1" />
                  {item.badge > 0 && (
                    <span
                      className="w-5 h-5 rounded-full bg-amber-500 text-white text-[10px] font-bold flex items-center justify-center shadow-[0_0_8px_rgba(245,158,11,0.4)]"
                      aria-label={`${item.badge} unresolved`}
                    >
                      {item.badge > 9 ? '9+' : item.badge}
                    </span>
                  )}
                </>
              )}
            </NavLink>
          ))}
        </nav>

        {/* Global Background Task Indicator */}
        {bgTasks.length > 0 && (
          <div
            className="mb-4 p-3 rounded-md bg-secondary border border-border flex items-center gap-3"
            role="status"
            aria-live="polite"
            aria-label={`Background task: ${bgTasks[0].message}`}
            data-testid="bg-task-indicator"
          >
            <Loader2 className="w-4 h-4 animate-spin text-muted-foreground shrink-0" aria-hidden="true" />
            <div className="text-xs text-muted-foreground truncate">
              {bgTasks[0].message}
            </div>
          </div>
        )}

        {/* Status Indicator */}
        <div
          className="mt-auto p-4 rounded-md bg-background border border-border flex items-center gap-3"
          role="status"
          aria-label={`Core engine status: ${backendStatus === 'healthy' ? 'Connected' : 'Offline'}`}
          data-testid="core-engine-status"
        >
          {backendStatus === 'healthy' ? (
            <div className="w-4 h-4 rounded-full bg-green-500 shadow-sm" aria-hidden="true" />
          ) : (
            <WifiOff size={16} className="text-red-700" aria-hidden="true" />
          )}
          <div>
            <div className="text-sm font-medium">Core Engine</div>
            <div
              className={cn(
                'text-xs',
                backendStatus === 'healthy' ? 'text-emerald-700' : 'text-red-700'
              )}
            >
              {backendStatus === 'healthy' ? 'Connected' : 'Offline'}
            </div>
          </div>
        </div>
      </aside>

      {/* Main Content Area */}
      <main className="flex-1 overflow-y-auto relative p-10" id="main-content">
        {/* Decorative background glow */}
        <div className="absolute -top-[10%] -right-[5%] w-[400px] h-[400px] bg-blue-600/15 blur-[150px] -z-10 pointer-events-none rounded-full" aria-hidden="true" />

        <div className="relative z-10 max-w-6xl mx-auto">
          {/* TASK-FE-016: a persistent banner, not a scrim over the routed
              content -- every route's real content must stay visible while
              locked (Doc 30: "still allowing navigation to read-only
              views"), and the backend's assert_write_allowed is what
              actually enforces the write-gate, not this component. */}
          <LicenseLockOverlay />
          <GracePeriodBanner />
          <StatementOnlyModeBanner />
          {/* Per-route error boundary so one page crash doesn't kill the whole shell */}
          <ErrorBoundary>
            <Outlet />
          </ErrorBoundary>
        </div>
      </main>
    </div>
  );
}
