import { useState, useEffect } from 'react';
import { AlertTriangle, Loader2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useLicenseStore } from '@/stores/useLicenseStore';
import { API } from '@/lib/ipc';

/**
 * TASK-FE-016 (Doc 30): "dismissable-but-recurring, showing days remaining
 * in the offline grace window and a 'Retry validation now' button wired to
 * license_validate [real name: license_refresh, Document 19 §14.4 --
 * license_validate doesn't exist]." Reactive off `useLicenseStore`.
 *
 * "Dismissable-but-recurring": dismissal is local component state, not
 * persisted -- it reappears on the next app launch/remount, and
 * immediately if the license leaves and re-enters GRACE (state !== 'GRACE'
 * resets the dismissed flag), rather than being permanently suppressed by
 * a single click the way a one-time toast would be.
 */
export default function GracePeriodBanner() {
  const state = useLicenseStore((s) => s.state);
  const daysRemainingInTrial = useLicenseStore((s) => s.daysRemainingInTrial);
  const [dismissed, setDismissed] = useState(false);
  const [isRetrying, setIsRetrying] = useState(false);

  const isGrace = state === 'GRACE';

  useEffect(() => {
    if (!isGrace) setDismissed(false);
  }, [isGrace]);

  if (!isGrace || dismissed) return null;

  const handleRetry = async () => {
    setIsRetrying(true);
    try {
      await API.licensing.refresh();
    } catch (err) {
      console.error('License refresh failed:', err);
    } finally {
      setIsRetrying(false);
    }
  };

  return (
    <div
      role="status"
      className="flex items-center gap-3 px-4 py-3 mb-4 rounded-lg border border-amber-500/30 bg-amber-500/10 text-sm"
    >
      <AlertTriangle className="w-4 h-4 text-amber-600 shrink-0" aria-hidden="true" />
      <p className="flex-1 text-amber-800">
        Your subscription is in its grace period
        {daysRemainingInTrial != null
          ? ` — ${daysRemainingInTrial} day${daysRemainingInTrial === 1 ? '' : 's'} remaining`
          : ''}
        . Resolve your payment to avoid losing access.
      </p>
      <Button variant="outline" size="sm" onClick={handleRetry} disabled={isRetrying} aria-label="Retry validation now">
        {isRetrying ? <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" /> : 'Retry validation now'}
      </Button>
      <button
        onClick={() => setDismissed(true)}
        aria-label="Dismiss grace period notice"
        className="text-amber-700 hover:text-amber-900"
      >
        <X className="w-4 h-4" aria-hidden="true" />
      </button>
    </div>
  );
}
