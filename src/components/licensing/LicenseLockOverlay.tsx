import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { AlertTriangle, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useLicenseStore } from '@/stores/useLicenseStore';
import { API } from '@/lib/ipc';

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
 * entire app (sidebar included) rather than allowing read-only navigation,
 * contradicting this task's own spec. This component is mounted inside the
 * routed content area (not at the true page root), so the sidebar stays
 * outside its bounds and remains clickable -- "full-screen" here means the
 * full content pane, not the full viewport including navigation.
 *
 * No specific lock *reason* (clock skew vs. grace expiry vs. invalid JWT)
 * is exposed by `LicenseStatusResponse` -- only `state: "LOCKED"` -- so the
 * copy here is deliberately generic rather than fabricating a reason the
 * backend never reports.
 */
export default function LicenseLockOverlay() {
  const isLocked = useLicenseStore((s) => s.isLocked);
  const navigate = useNavigate();
  const [isRetrying, setIsRetrying] = useState(false);

  if (!isLocked) return null;

  const handleRetry = async () => {
    setIsRetrying(true);
    try {
      await API.licensing.refresh();
      // A successful refresh emits license_state_changed, which
      // useLicenseStore mirrors -- isLocked flips to false reactively,
      // dismissing this overlay without any local state change here.
    } catch (err) {
      console.error('License refresh failed:', err);
    } finally {
      setIsRetrying(false);
    }
  };

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="license-locked-title"
      aria-describedby="license-locked-desc"
      className="absolute inset-0 z-40 flex flex-col items-center justify-center bg-background/95 backdrop-blur-sm p-8"
    >
      <div className="max-w-md w-full bg-card border border-border rounded-lg p-6 flex flex-col items-center text-center space-y-4">
        <div className="h-12 w-12 rounded-full bg-amber-500/20 flex items-center justify-center">
          <AlertTriangle className="text-amber-500 w-6 h-6" aria-hidden="true" />
        </div>
        <h2 id="license-locked-title" className="text-xl font-semibold">License Locked</h2>
        <p id="license-locked-desc" className="text-sm text-muted-foreground">
          Your license could not be validated. Editing and syncing are paused until you reactivate
          or your license revalidates — you can still browse your existing data.
        </p>
        <div className="flex flex-col gap-2 w-full">
          <Button variant="default" onClick={() => navigate('/settings')} aria-label="Go to Settings to reactivate">
            Reactivate in Settings
          </Button>
          <Button variant="outline" onClick={handleRetry} disabled={isRetrying} aria-label="Retry license validation now">
            {isRetrying ? (
              <><Loader2 className="w-4 h-4 mr-2 animate-spin" aria-hidden="true" /> Checking...</>
            ) : (
              'Retry Validation'
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
