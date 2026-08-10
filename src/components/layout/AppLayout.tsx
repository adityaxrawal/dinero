/**
 * The application shell: persistent sidebar plus a routed main content area.
 *
 * Rendered once around every authenticated route, so anything mounted here
 * lives for the whole session -- which is exactly what the banners, overlays and
 * notification centre need in order to keep reporting background work while the
 * user navigates between screens.
 *
 * One branch short-circuits the entire layout: a corrupted database replaces the
 * shell with a recovery screen, because navigation and data-backed panels cannot
 * function until the user chooses to restore a backup or start fresh.
 */
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

/**
 * Drives the one-shot pulse on the reconciliation badge.
 *
 * The store latches the flag on; this hook clears it after the animation window
 * so the pulse plays once per trigger instead of animating continuously. The
 * timer is cleared on unmount to avoid setting state on a gone component.
 */
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

/** Sidebar footer indicator showing whether the Rust backend is responding. */
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

/** The application shell: persistent sidebar plus a routed content area. */
export default function AppLayout() {
  const status = useBackendStatus();
  const badgePulse = useBadgePulse();
  const isActive = useIsActive();

  const { data: reconciliationClusters = [] } = useReconciliationClusters();

  // Recovery takes over the whole window. Rendering the normal shell over a
  // corrupted database would offer navigation to screens that cannot load.
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
      {/* Skip link: visually hidden until focused, giving keyboard and screen
          reader users a way past the sidebar straight to the content. */}
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:absolute focus:top-4 focus:left-14 focus:z-50 focus:px-4 focus:py-2 focus:rounded-lg focus:text-sm focus:font-medium focus:shadow-lg"
        style={{ backgroundColor: '#F8E7C9', border: '1px solid #064E3B', color: '#064E3B' }}
      >
        Skip to main content
      </a>

      {/* Sidebar: fixed-width navigation column, laid out top to bottom as
          brand, primary nav, system messages, then the status footer. */}
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

        {/* System messages (Gmail disconnected, license grace/locked, and
            similar). These sit in the normal flow rather than floating over the
            routed content, so a persistent warning never obscures the screen the
            user is trying to work on. Each banner decides for itself whether it
            has anything to show. */}
        <div className="flex flex-col flex-shrink-0" aria-label="System messages">
          <LicenseLockOverlay />
          <GracePeriodBanner />
          <StatementOnlyModeBanner />
          <ConnectionStatusBanner />
          <AlertBanner />
        </div>

        {/* Bottom area (System & Status). `mt-auto` pins this block to the
            bottom of the sidebar regardless of how tall the nav above it is.
            SidebarNotificationCenter is the single owner of scan and
            background-task progress -- it must stay inside this block. */}
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

      {/* Main content area: the routed screen fills whatever space the sidebar
          leaves, managing its own internal columns. */}
      <main id="main-content" className="flex-1 flex overflow-hidden relative">
        {/* The boundary wraps only the Outlet, so a render crash in one screen
            leaves the sidebar intact and the app still navigable. */}
        <ErrorBoundary>
          <Outlet />
        </ErrorBoundary>

        {/* Positioned last and absolutely, so it covers the content area when an
            OS-level permission has been denied. */}
        <PermissionDeniedOverlay />
      </main>
    </div>
  );
}
