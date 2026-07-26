import { useState, useEffect, useCallback } from 'react';
import { NavLink, Outlet, useNavigate, useLocation } from 'react-router-dom';
import {
  LayoutDashboard,
  ArrowLeftRight,
  CreditCard,
  FileText,
  GitMerge,
  Settings,
  Activity,
  AlertTriangle,
  Loader2,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { isTauriRuntime } from '@/lib/tauriRuntime';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { cn } from '@/lib/utils';
import { API } from '@/lib/ipc';
import { useLicenseStore } from '@/stores/useLicenseStore';
import LicenseLockOverlay from '@/components/licensing/LicenseLockOverlay';
import GracePeriodBanner from '@/components/licensing/GracePeriodBanner';
import StatementOnlyModeBanner from '@/components/shell/StatementOnlyModeBanner';
import BackgroundTaskIndicator from '@/components/shell/BackgroundTaskIndicator';
import ScanStatusSidebarItem from '@/components/layout/ScanStatusSidebarItem';
import PermissionDeniedOverlay from '@/components/shell/PermissionDeniedOverlay';
import ConnectionStatusBanner from '@/components/notifications/ConnectionStatusBanner';
import AlertBanner from '@/components/notifications/AlertBanner';
import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { useReconciliationNudgeStore } from '@/stores/useReconciliationNudgeStore';

const CORRUPTION_EVENT = 'db_corrupted';

interface NavItem {
  path: string;
  label: string;
  icon: React.ReactNode;
  badge?: number;
  /** TASK-RT-006: a brief attention pulse on the badge itself -- suppressed
   * during an active bulk scan and fired once, in aggregate, on
   * `scan_completed` instead (`useReconciliationNudgeStore`). */
  pulse?: boolean;
}

/** Minimal SVG logo mark used in the rail */
function LogoMark() {
  return (
    <svg
      width="20"
      height="20"
      viewBox="0 0 512 512"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <rect x="72" y="82" rx="22" ry="22" width="368" height="110" fill="#F8E7C9" />
      <rect x="72" y="214" rx="22" ry="22" width="368" height="216" fill="#F8E7C9" />
      <rect x="110" y="112" width="146" height="22" rx="6" fill="#064E3B" />
      <rect x="274" y="112" width="88" height="22" rx="6" fill="rgba(6,78,59,0.5)" />
      <path
        d="M132 355 L192 295 L252 340 L336 256"
        fill="none"
        stroke="#064E3B"
        strokeWidth="16"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function SidebarItem({ item, isActive }: { item: NavItem; isActive: boolean }) {
  return (
    <NavLink
      to={item.path}
      end={item.path === '/'}
      className="relative block w-full px-3"
      aria-label={
        item.badge && item.badge > 0 ? `${item.label} — ${item.badge} pending` : item.label
      }
    >
      <span
        className={cn(
          'relative flex items-center rounded-lg transition-all px-3 py-1.5 w-full',
          isActive
            ? 'bg-[#F8E7C9] text-[#064E3B] font-semibold shadow-sm'
            : 'text-[#F8E7C9]/70 hover:text-[#F8E7C9] hover:bg-[#F8E7C9]/10 font-medium'
        )}
        aria-current={isActive ? 'page' : undefined}
      >
        <span className="flex-shrink-0 flex items-center justify-center w-6">{item.icon}</span>

        <span className="ml-2 text-[13px] whitespace-nowrap overflow-hidden">{item.label}</span>

        {item.badge != null && item.badge > 0 && (
          <span
            className={cn(
              'flex items-center justify-center text-[10px] font-bold rounded-full ml-auto px-2 min-w-[20px] h-5 transition-transform duration-300',
              isActive ? 'bg-[#064E3B]/10 text-[#064E3B]' : 'bg-[#f59e0b] text-white',
              item.pulse && 'scale-125'
            )}
            data-testid={item.pulse !== undefined ? 'reconciliation-badge' : undefined}
            data-pulsing={item.pulse}
            aria-hidden="true"
          >
            {item.badge > 9 ? '9+' : item.badge}
          </span>
        )}
      </span>
    </NavLink>
  );
}

export default function AppLayout() {
  const navigate = useNavigate();
  const location = useLocation();
  const [backendStatus, setBackendStatus] = useState<'healthy' | 'offline' | 'corrupted'>(
    'healthy'
  );
  const hydrateLicenseStore = useLicenseStore((s) => s.hydrate);

  // TASK-RT-006: React Query already auto-invalidates this on every
  // `reconciliation_cluster` event (`useIpcQueryInvalidation.ts`), so the
  // count itself live-increments with no polling and no manual re-fetch --
  // previously fetched once on mount and never updated again.
  const { data: reconciliationClusters = [] } = useReconciliationClusters();
  const unresolvedClusters = reconciliationClusters.length;
  const badgePulse = useReconciliationNudgeStore((s) => s.justPulsed);
  const clearBadgePulse = useReconciliationNudgeStore((s) => s.clearPulse);
  useEffect(() => {
    if (!badgePulse) return;
    const timer = setTimeout(clearBadgePulse, 600);
    return () => clearTimeout(timer);
  }, [badgePulse, clearBadgePulse]);

  const [isRestoring, setIsRestoring] = useState(false);
  const [isStartingFresh, setIsStartingFresh] = useState(false);

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

  const handleStartFresh = useCallback(async () => {
    let confirmed: boolean;
    const warning =
      'Start fresh? This permanently deletes all local data (transactions, statements, settings) and returns you to onboarding. This cannot be undone.';
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
    if (isTauriRuntime() && !localStorage.getItem('dinero_onboarded')) {
      navigate('/onboarding', { replace: true });
      return;
    }

    API.status
      .check()
      .then((result) =>
        setBackendStatus((prev) =>
          prev === 'healthy' || prev === 'offline' ? (result.status as 'healthy') : prev
        )
      )
      .catch(() =>
        setBackendStatus((prev) => (prev === 'healthy' || prev === 'offline' ? 'offline' : prev))
      );

    hydrateLicenseStore();

    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      let listen;
      try {
        const m = await import('@tauri-apps/api/event');
        listen = m.listen;
      } catch {
        return;
      }
      if (!listen) return;

      const unlistenCorrupt = await listen(CORRUPTION_EVENT, () => {
        setBackendStatus('corrupted');
      });
      unlisteners.push(unlistenCorrupt);

      // Doc 30 TASK-RT-003: `alert_threshold_crossed` handling (toast +
      // persistent banner) moved to `useAlertStore.ts`'s own module-load
      // subscription, alongside the other event-store patterns
      // (`useSyncStore.ts`) -- this previously listened here with a
      // fabricated `{category, threshold}` payload shape that never
      // matched the real `{transaction_id, alert_type, message}` the
      // backend emits, and only `console.log`'d it.
    };

    setup().catch(console.error);
    return () => unlisteners.forEach((fn) => fn());
  }, [navigate, hydrateLicenseStore]);

  const navGroups = [
    {
      title: 'Workspace',
      items: [
        { path: '/', label: 'Command Center', icon: <LayoutDashboard size={15} />, badge: 0 },
        {
          path: '/reconciliation',
          label: 'Review Inbox',
          icon: <GitMerge size={15} />,
          badge: unresolvedClusters,
          pulse: badgePulse,
        },
        { path: '/statements', label: 'Statements', icon: <FileText size={15} />, badge: 0 },
      ],
    },
    {
      title: 'Ledger',
      items: [
        {
          path: '/transactions',
          label: 'All Transactions',
          icon: <ArrowLeftRight size={15} />,
          badge: 0,
        },
        { path: '/instruments', label: 'Accounts', icon: <CreditCard size={15} />, badge: 0 },
      ],
    },
  ];

  const systemItems: NavItem[] = [
    { path: '/settings', label: 'Settings', icon: <Settings size={15} />, badge: 0 },
    ...(import.meta.env.DEV
      ? [{ path: '/debug', label: 'Debug', icon: <Activity size={15} />, badge: 0 }]
      : []),
  ];

  /* ── Corrupted DB Recovery ──────────────────────────────── */
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
            backgroundColor: 'hsl(38, 55%, 91%)',
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
              The SQLite integrity check failed. Restore from a backup to recover your data, or
              start fresh.
            </p>
          </div>
          <div className="flex flex-col gap-3 w-full">
            <Button
              onClick={handleRestoreBackup}
              disabled={isRestoring || isStartingFresh}
              aria-label="Restore database from backup"
              className="w-full font-semibold"
              style={{ backgroundColor: '#064E3B', color: '#F8E7C9' }}
            >
              {isRestoring ? (
                <>
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Restoring…
                </>
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
                <>
                  <Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Starting
                  Fresh…
                </>
              ) : (
                'Start Fresh (Delete All Data)'
              )}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  /* ── Main App Shell ─────────────────────────────────────── */
  return (
    <div className="flex h-screen w-screen overflow-hidden" style={{ backgroundColor: '#F8E7C9' }}>
      {/* Skip link */}
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-14 focus:z-50 focus:px-4 focus:py-2 focus:rounded-lg focus:text-sm focus:font-medium focus:shadow-lg"
        style={{ backgroundColor: '#F8E7C9', border: '1px solid #064E3B', color: '#064E3B' }}
      >
        Skip to main content
      </a>

      {/* ── Column 1: Sidebar ──────────────────────────────── */}
      <aside
        className="flex-shrink-0 z-40 flex flex-col border-r border-[#064E3B]/20"
        style={{ width: '256px', backgroundColor: '#064E3B' }}
        aria-label="Main navigation"
      >
        {/* Logo area */}
        <div className="flex items-center px-6 h-14 flex-shrink-0 mt-2">
          <div
            className="flex items-center justify-center rounded-lg flex-shrink-0"
            style={{ width: '28px', height: '28px', backgroundColor: 'rgba(248,231,201,0.12)' }}
          >
            <LogoMark />
          </div>
          <span
            className="ml-3 font-semibold text-[15px] tracking-tight"
            style={{ color: '#F8E7C9' }}
          >
            Dinero
          </span>
        </div>

        {/* Nav items */}
        <nav
          className="flex-1 flex flex-col w-full py-4 gap-6 overflow-y-auto"
          aria-label="Primary navigation"
        >
          {navGroups.map((group) => (
            <div key={group.title} className="flex flex-col gap-1">
              <div className="px-6 text-[10px] font-semibold uppercase tracking-wider text-[#F8E7C9]/40 mb-1">
                {group.title}
              </div>
              {group.items.map((item) => {
                const isActive =
                  item.path === '/'
                    ? location.pathname === '/'
                    : location.pathname.startsWith(item.path);
                return <SidebarItem key={item.path} item={item} isActive={isActive} />;
              })}
            </div>
          ))}
        </nav>

        {/* System messages (Gmail-disconnected, license grace/locked) —
            in-flow here, not an absolutely-positioned overlay on top of
            routed content (see StatementOnlyModeBanner's doc comment). */}
        <div className="flex flex-col flex-shrink-0" aria-label="System messages">
          <LicenseLockOverlay />
          <GracePeriodBanner />
          <StatementOnlyModeBanner />
          <ConnectionStatusBanner />
          <AlertBanner />
        </div>

        {/* Bottom area (System & Status) */}
        <div className="mt-auto flex flex-col">
          <ScanStatusSidebarItem />

          <div className="pb-4 pt-4 flex flex-col gap-1 border-t border-[#F8E7C9]/10">
            {systemItems.map((item) => {
              const isActive = location.pathname.startsWith(item.path);
              return <SidebarItem key={item.path} item={item} isActive={isActive} />;
            })}

          {/* Status Indicator */}
          <div className="px-6 mt-4 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div
                className={cn(
                  'w-2 h-2 rounded-full',
                  backendStatus === 'healthy' ? 'bg-[#10b981]' : 'bg-[#ef4444]'
                )}
              />
              <span className="text-[11px] font-medium text-[#F8E7C9]/60">
                {backendStatus === 'healthy' ? 'Engine Online' : 'Engine Offline'}
              </span>
            </div>
          </div>
          </div>
        </div>
      </aside>

      {/* ── Main Content Area ──────────────────────────────── */}
      <main id="main-content" className="flex-1 flex overflow-hidden relative">
        {/* Outlet acts as Column 2 and Column 3 (or full canvas) */}
        <ErrorBoundary>
          <Outlet />
        </ErrorBoundary>

        {/* Background task indicator */}
        <div className="absolute bottom-4 right-4 z-30">
          <BackgroundTaskIndicator />
        </div>

        {/* OS permission denied overlay */}
        <PermissionDeniedOverlay />
      </main>
    </div>
  );
}
