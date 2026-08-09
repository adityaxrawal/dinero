import { useEffect } from 'react';
import { Outlet } from 'react-router-dom';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { cn } from '@/lib/utils';
import LicenseLockOverlay from '@/components/licensing/LicenseLockOverlay';
import GracePeriodBanner from '@/components/licensing/GracePeriodBanner';
import StatementOnlyModeBanner from '@/components/shell/StatementOnlyModeBanner';
import SidebarNotificationCenter from '@/components/layout/SidebarNotificationCenter';
import PermissionDeniedOverlay from '@/components/shell/PermissionDeniedOverlay';
import ConnectionStatusBanner from '@/components/notifications/ConnectionStatusBanner';
import AlertBanner from '@/components/notifications/AlertBanner';
import { useReconciliationClusters } from '@/hooks/queries/useReconciliationClusters';
import { useReconciliationNudgeStore } from '@/stores/useReconciliationNudgeStore';
import SidebarNav from './appShell/SidebarNav';
import LogoMark from './appShell/LogoMark';
import SidebarItem from './appShell/SidebarItem';
import { SYSTEM_ITEMS, useIsActive } from './appShell/navItems';
import CorruptedDbScreen from './appShell/CorruptedDbScreen';
import { useBackendStatus } from './appShell/useBackendStatus';

/** TASK-RT-006: a one-shot badge pulse, cleared after the animation. */
function useBadgePulse() {
  const badgePulse = useReconciliationNudgeStore((s) => s.justPulsed);
  const clearBadgePulse = useReconciliationNudgeStore((s) => s.clearPulse);
  useEffect(() => {
    if (!badgePulse) return;
    const timer = setTimeout(clearBadgePulse, 600);
    return () => clearTimeout(timer);
  }, [badgePulse, clearBadgePulse]);
  return badgePulse;
}

function EngineStatus({ healthy }: { healthy: boolean }) {
  return (
    <div className="px-6 mt-4 flex items-center justify-between">
      <div className="flex items-center gap-2">
        <div className={cn('w-2 h-2 rounded-full', healthy ? 'bg-[#10b981]' : 'bg-[#ef4444]')} />
        <span className="text-[11px] font-medium text-[#F8E7C9]/60">
          {healthy ? 'Engine Online' : 'Engine Offline'}
        </span>
      </div>
    </div>
  );
}

export default function AppLayout() {
  const status = useBackendStatus();
  const badgePulse = useBadgePulse();
  const isActive = useIsActive();

  // TASK-RT-006: React Query already auto-invalidates this on every
  // `reconciliation_cluster` event (`useIpcQueryInvalidation.ts`), so the
  // count itself live-increments with no polling and no manual re-fetch.
  const { data: reconciliationClusters = [] } = useReconciliationClusters();

  if (status.backendStatus === 'corrupted') {
    return (
      <CorruptedDbScreen
        isRestoring={status.isRestoring}
        isStartingFresh={status.isStartingFresh}
        onRestore={status.handleRestoreBackup}
        onStartFresh={status.handleStartFresh}
      />
    );
  }

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

        <SidebarNav
          unresolvedClusters={reconciliationClusters.length}
          badgePulse={badgePulse}
        />

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
          <SidebarNotificationCenter />

          <div className="pb-4 pt-4 flex flex-col gap-1 border-t border-[#F8E7C9]/10">
            {SYSTEM_ITEMS.map((item) => (
              <SidebarItem key={item.path} item={item} isActive={isActive(item.path)} />
            ))}
            <EngineStatus healthy={status.backendStatus === 'healthy'} />
          </div>
        </div>
      </aside>

      {/* ── Main Content Area ──────────────────────────────── */}
      <main id="main-content" className="flex-1 flex overflow-hidden relative">
        {/* Outlet acts as Column 2 and Column 3 (or full canvas) */}
        <ErrorBoundary>
          <Outlet />
        </ErrorBoundary>

        {/* OS permission denied overlay */}
        <PermissionDeniedOverlay />
      </main>
    </div>
  );
}
