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
 *
 * Rendered in the sidebar's "Messages" section (`AppLayout.tsx`) — see
 * `StatementOnlyModeBanner`'s doc comment for why it moved off routed
 * content entirely (a `position: absolute` overlap bug, not a design choice
 * to revert).
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
      className="flex flex-col gap-2 mx-4 mb-2 px-3 py-2.5 rounded-lg border border-red-400/30 bg-red-400/10"
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className="w-3.5 h-3.5 text-red-300 shrink-0 mt-0.5" aria-hidden="true" />
        <div className="flex-1 text-[11.5px] leading-snug">
          <span id="license-locked-title" className="font-semibold text-red-200">
            License Locked.
          </span>{' '}
          <span id="license-locked-desc" className="text-red-200/80">
            Editing and syncing are paused until you reactivate — you can still browse existing
            data.
          </span>
        </div>
      </div>
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={handleRetry}
          disabled={isRetrying}
          aria-label="Retry license validation now"
          className="h-7 text-[11.5px] flex-1 border-red-400/30 text-red-200 bg-transparent hover:bg-red-400/10 hover:text-red-100"
        >
          {isRetrying ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" aria-hidden="true" />
          ) : (
            'Retry'
          )}
        </Button>
        <Button
          variant="default"
          size="sm"
          onClick={() => navigate('/settings')}
          aria-label="Go to Settings to reactivate"
          className="h-7 text-[11.5px] flex-1 bg-[#F8E7C9] text-[#064E3B] hover:bg-[#F8E7C9]/90"
        >
          Reactivate
        </Button>
      </div>
    </div>
  );
}
