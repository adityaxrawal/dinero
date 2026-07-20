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
import BackgroundTaskIndicator from '@/components/shell/BackgroundTaskIndicator';
import PermissionDeniedOverlay from '@/components/shell/PermissionDeniedOverlay';

// HMR trigger 3
const CORRUPTION_EVENT = 'db_corrupted';

export default function AppLayout() {
  const navigate = useNavigate();
  const [backendStatus, setBackendStatus] = useState<'healthy' | 'offline' | 'corrupted'>('healthy');
  const hydrateLicenseStore = useLicenseStore((s) => s.hydrate);
  // Transient "spend threshold crossed" notice -- distinct from, and no
  // longer piggybacking on, the background-task indicator state below
  // (TASK-DESK-003: a spend alert isn't a long-running task and shouldn't
  // share its aggregation logic).
  const [alertMessage, setAlertMessage] = useState<string | null>(null);
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

      // TASK-DESK-003: the global background-task indicator now owns the
      // `background_task_progress` event itself (see
      // `BackgroundTaskIndicator`) -- this effect only needs the unrelated
      // spend-threshold alert listener below.
      const unlistenAlert = await listen('alert_threshold_crossed', (event: { payload: any }) => {
        setAlertMessage(`Alert: ${event.payload.category} exceeded ${event.payload.threshold}% of budget`);
        setTimeout(() => setAlertMessage(null), 5000);
      });
      unlisteners.push(unlistenAlert);
    };

    setup().catch(console.error);

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [navigate, hydrateLicenseStore]);

  const navItems = [
    { path: '/', label: 'Dashboard', icon: <LayoutDashboard size={18} />, badge: 0 },
    { path: '/transactions', label: 'Transactions', icon: <ArrowLeftRight size={18} />, badge: 0 },
    { path: '/instruments', label: 'Instruments', icon: <CreditCard size={18} />, badge: 0 },
    { path: '/statements', label: 'Statements', icon: <FileText size={18} />, badge: 0 },
    {
      path: '/reconciliation',
      label: 'Reconciliation',
      icon: <GitMerge size={18} />,
      badge: unresolvedClusters,
    },
    // G16 fix: moved into a Settings subsection rather than a top-level nav
    // item — the route ('/spending-limits') still exists and is linked from
    // Settings, it's just no longer competing for sidebar space.
    { path: '/settings', label: 'Settings', icon: <Settings size={18} />, badge: 0 },
    // F14 fix: Debug Console must not be reachable in production builds.
    ...(import.meta.env.DEV
      ? [{ path: '/debug', label: 'Debug', icon: <Activity size={18} />, badge: 0 }]
      : []),
  ];

  /* ── Corrupted DB Recovery Screen (Doc 13 Flow 4.17) ──────── */
  if (backendStatus === 'corrupted') {
    return (
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="db-corrupted-title"
        aria-describedby="db-corrupted-desc"
        className="flex flex-col items-center justify-center h-screen p-8"
        style={{ backgroundColor: '#F8E7C9' }}
      >
        <div
          className="max-w-md w-full rounded-2xl p-8 flex flex-col items-center text-center gap-5"
          style={{
            backgroundColor: '#fdf6ed',
            border: '1px solid #d9c8a8',
            boxShadow: '0 4px 24px rgba(6,78,59,0.10)',
          }}
        >
          <div
            className="h-14 w-14 rounded-2xl flex items-center justify-center"
            style={{ backgroundColor: 'rgba(239,68,68,0.10)' }}
          >
            <AlertTriangle className="w-7 h-7" style={{ color: '#dc2626' }} aria-hidden="true" />
          </div>
          <div>
            <h2
              id="db-corrupted-title"
              className="text-xl font-semibold mb-2"
              style={{ color: '#0d2b22' }}
            >
              Database Corrupted
            </h2>
            <p id="db-corrupted-desc" className="text-sm" style={{ color: '#3d5a50' }}>
              The SQLite integrity check failed. Restore from a backup to recover your data, or start fresh.
            </p>
          </div>
          <div className="flex flex-col gap-3 w-full">
            <Button
              onClick={handleRestoreBackup}
              disabled={isRestoring || isStartingFresh}
              aria-label="Restore database from backup"
              className="w-full font-semibold"
              style={{
                backgroundColor: '#064E3B',
                color: '#F8E7C9',
              }}
            >
              {isRestoring ? (
                <><Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Restoring…</>
              ) : (
                'Restore from Backup'
              )}
            </Button>
            <Button
              variant="outline"
              onClick={handleStartFresh}
              disabled={isRestoring || isStartingFresh}
              aria-label="Delete all local data and start fresh"
              className="w-full"
              style={{ borderColor: '#d9c8a8', color: '#3d5a50' }}
            >
              {isStartingFresh ? (
                <><Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Starting Fresh…</>
              ) : (
                'Start Fresh (Delete All Data)'
              )}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  /* ── Main App Shell ──────────────────────────────────────── */
  return (
    <div
      className="flex h-screen w-screen overflow-hidden"
      style={{ backgroundColor: '#F8E7C9' }}
    >
      {/* Skip to main content — keyboard accessibility (Doc 14 §10) */}
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-4 focus:z-50 focus:px-4 focus:py-2 focus:rounded-lg focus:text-sm focus:font-medium focus:shadow-lg"
        style={{
          backgroundColor: '#F8E7C9',
          border: '1px solid #064E3B',
          color: '#064E3B',
        }}
      >
        Skip to main content
      </a>

      {/* ── Sidebar — Emerald Ink ─────────────────────────── */}
      <aside
        className="flex flex-col z-10 flex-shrink-0"
        style={{
          width: '240px',
          backgroundColor: '#064E3B',
          borderRight: '1px solid rgba(248,231,201,0.12)',
        }}
        aria-label="Main navigation"
      >
        {/* Logo + Wordmark */}
        <div
          className="flex items-center gap-3 px-6"
          style={{ height: '68px', borderBottom: '1px solid rgba(248,231,201,0.10)' }}
        >
          <div
            className="flex items-center justify-center rounded-xl flex-shrink-0"
            style={{
              width: '34px',
              height: '34px',
              backgroundColor: 'rgba(248,231,201,0.12)',
            }}
          >
            {/* Primary Logo SVG — champagne on emerald */}
            <svg width="20" height="20" viewBox="0 0 512 512" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
              <rect x="72" y="82" rx="22" ry="22" width="368" height="110" fill="#F8E7C9"/>
              <rect x="72" y="214" rx="22" ry="22" width="368" height="216" fill="#F8E7C9"/>
              <rect x="110" y="112" width="146" height="22" rx="6" fill="#064E3B"/>
              <rect x="274" y="112" width="88" height="22" rx="6" fill="rgba(6,78,59,0.5)"/>
              <path d="M132 355 L192 295 L252 340 L336 256" fill="none" stroke="#064E3B" strokeWidth="16" strokeLinecap="round" strokeLinejoin="round"/>
            </svg>
          </div>
          <span
            className="text-lg font-semibold tracking-tight select-none"
            style={{ color: '#F8E7C9', letterSpacing: '-0.01em' }}
          >
            Dinero
          </span>
        </div>

        {/* Navigation */}
        <nav className="flex-1 px-3 py-4 flex flex-col gap-0.5" aria-label="Primary navigation">
          {navItems.map((item) => (
            <NavLink
              key={item.path}
              to={item.path}
              end={item.path === '/'}
              aria-label={item.badge > 0 ? `${item.label} — ${item.badge} pending` : item.label}
              className={({ isActive }) =>
                cn(
                  'relative flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all duration-200 ease-out outline-none group',
                  isActive
                    ? 'text-[#064E3B]'
                    : 'text-[rgba(248,231,201,0.72)] hover:text-[#F8E7C9]'
                )
              }
              style={({ isActive }) =>
                isActive
                  ? {
                      backgroundColor: '#F8E7C9',
                      color: '#064E3B',
                    }
                  : {
                      backgroundColor: 'transparent',
                    }
              }
              onMouseEnter={(e) => {
                const el = e.currentTarget;
                if (!el.getAttribute('aria-current')) {
                  el.style.backgroundColor = 'rgba(248,231,201,0.10)';
                }
              }}
              onMouseLeave={(e) => {
                const el = e.currentTarget;
                if (!el.getAttribute('aria-current')) {
                  el.style.backgroundColor = 'transparent';
                }
              }}
            >
              {({ isActive }) => (
                <>
                  {/* Icon */}
                  <span
                    className="shrink-0 transition-colors duration-200"
                    aria-hidden="true"
                    style={{ color: isActive ? '#064E3B' : 'inherit' }}
                  >
                    {item.icon}
                  </span>
                  {item.label}
                  <div className="flex-1" />
                  {item.badge > 0 && (
                    <span
                      className="min-w-[20px] h-5 px-1.5 rounded-full text-[10px] font-bold flex items-center justify-center"
                      style={{
                        backgroundColor: isActive ? '#064E3B' : '#f59e0b',
                        color: isActive ? '#F8E7C9' : '#fff',
                      }}
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

        {/* Background Task Indicator (TASK-DESK-003) */}
        <BackgroundTaskIndicator />

        {/* OS-level permission denial states (TASK-DESK-004) */}
        <PermissionDeniedOverlay />

        {/* Transient spend-threshold alert notice */}
        {alertMessage && (
          <div
            className="mx-3 mb-3 p-3 rounded-lg flex items-center gap-3"
            style={{
              backgroundColor: 'rgba(245,158,11,0.15)',
              border: '1px solid rgba(245,158,11,0.30)',
            }}
            role="status"
            aria-live="polite"
            data-testid="alert-notice"
          >
            <div className="text-xs truncate" style={{ color: '#fde68a' }}>{alertMessage}</div>
          </div>
        )}

        {/* Core Engine Status */}
        <div
          className="mx-3 mb-4 p-3 rounded-xl flex items-center gap-3"
          style={{
            backgroundColor: 'rgba(248,231,201,0.07)',
            border: '1px solid rgba(248,231,201,0.10)',
          }}
          role="status"
          aria-label={`Core engine status: ${backendStatus === 'healthy' ? 'Connected' : 'Offline'}`}
          data-testid="core-engine-status"
        >
          {backendStatus === 'healthy' ? (
            <div
              className="w-2 h-2 rounded-full flex-shrink-0"
              style={{ backgroundColor: '#10b981' }}
              aria-hidden="true"
            />
          ) : (
            <WifiOff size={14} style={{ color: '#ef4444' }} aria-hidden="true" />
          )}
          <div>
            <div className="text-xs font-medium" style={{ color: '#F8E7C9' }}>Core Engine</div>
            <div
              className="text-[10px]"
              style={{ color: backendStatus === 'healthy' ? '#6ee7b7' : '#fca5a5' }}
            >
              {backendStatus === 'healthy' ? 'Connected' : 'Offline'}
            </div>
          </div>
        </div>
      </aside>

      {/* ── Main Content Area ─────────────────────────────── */}
      <main
        className="flex-1 overflow-y-auto relative"
        id="main-content"
        style={{ backgroundColor: '#F8E7C9' }}
      >
        {/* Subtle warm canvas texture gradient */}
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            background: 'radial-gradient(ellipse at 70% 0%, rgba(6,78,59,0.04) 0%, transparent 60%)',
          }}
          aria-hidden="true"
        />

        <div className="relative p-8 max-w-6xl mx-auto">
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
