import { useNavigate } from 'react-router-dom';
import { AlertTriangle, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useLicenseStore } from '@/stores/useLicenseStore';
import { useLicenseRefresh } from '@/hooks/useLicenseRefresh';

/**
 * TASK-FE-016 (Doc 30): "full-screen, non-dismissable, rendered at the
 * AppShell level whenever isLocked === true, blocking write interactions
 * but explicitly still allowing navigation to read-only views (per
 * TASK-API-010's read/write license-gate distinction) and to the
 * reactivation flow." Reactive off `useLicenseStore`, so a background
 * revalidation dismisses it immediately without a page reload.
 *
 * Real gap this replaces: AppLayout previously had an ad-hoc "locked"
 * dialog driven by listening for a `license_clock_skew` event -- that
 * event is defined in the backend's event enum but never actually emitted
 * anywhere (grepped the whole crate), so that dialog could never appear in
 * production regardless of why the license was really locked (grace
 * expiry, JWT failure, etc., not just clock skew). It also replaced the
 * entire app (sidebar included).
 *
 * Area 9 verification-pass fix: the first rebuild of this component fixed
 * the sidebar-blocking bug above but introduced a narrower version of the
 * exact same defect -- it was an opaque `absolute inset-0` scrim mounted
 * inside `<main>`, so it covered every routed page's content, not just the
 * page it was first shown on. Clicking a sidebar link did change the URL,
 * but the destination page's content was still hidden behind the same
 * full-content-pane overlay -- "read-only views" were reachable in name
 * only, never actually visible. Since the real write-gate is already
 * enforced backend-side (`assert_write_allowed` fails closed on every
 * mutating command regardless of what the frontend shows), this component
 * only needs to *communicate* the lock, not visually block reading -- so
 * it's now a persistent, non-dismissable banner (same positioning pattern
 * as `GracePeriodBanner`, just non-dismissable and rendered above the
 * routed content rather than as a scrim over it) instead of a full-pane
 * overlay. Content underneath stays fully visible/scrollable on every
 * route.
 *
 * No specific lock *reason* (clock skew vs. grace expiry vs. invalid JWT)
 * is exposed by `LicenseStatusResponse` -- only `state: "LOCKED"` -- so the
 * copy here is deliberately generic rather than fabricating a reason the
 * backend never reports.
 */
export default function LicenseLockOverlay() {
  const isLocked = useLicenseStore((s) => s.isLocked);
  const navigate = useNavigate();
  const { isRetrying, handleRetry } = useLicenseRefresh();

  if (!isLocked) return null;

  return (
    <div
      role="alert"
      aria-labelledby="license-locked-title"
      aria-describedby="license-locked-desc"
      className="flex items-center gap-3 px-4 py-3 mb-4 rounded-lg border border-red-700/30 bg-red-700/10 text-sm"
    >
      <AlertTriangle className="w-4 h-4 text-red-700 shrink-0" aria-hidden="true" />
      <div className="flex-1">
        <span id="license-locked-title" className="font-semibold text-red-700">License Locked</span>{' '}
        <span id="license-locked-desc" className="text-red-800">
          Your license could not be validated. Editing and syncing are paused until you reactivate
          or your license revalidates — you can still browse your existing data.
        </span>
      </div>
      <Button variant="outline" size="sm" onClick={handleRetry} disabled={isRetrying} aria-label="Retry license validation now">
        {isRetrying ? <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" /> : 'Retry Validation'}
      </Button>
      <Button variant="default" size="sm" onClick={() => navigate('/settings')} aria-label="Go to Settings to reactivate">
        Reactivate
      </Button>
    </div>
  );
}
