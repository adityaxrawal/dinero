import { useState, useEffect } from 'react';
import { AlertTriangle, Loader2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useLicenseStore } from '@/stores/useLicenseStore';
import { useLicenseRefresh } from '@/hooks/useLicenseRefresh';

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
 *
 * Rendered in the sidebar's "Messages" section (`AppLayout.tsx`) — see
 * `StatementOnlyModeBanner`'s doc comment for why it moved off routed
 * content entirely (a `position: absolute` overlap bug, not a design choice
 * to revert).
 */
export default function GracePeriodBanner() {
  const state = useLicenseStore((s) => s.state);
  const daysRemainingInTrial = useLicenseStore((s) => s.daysRemainingInTrial);
  const [dismissed, setDismissed] = useState(false);
  const { isRetrying, handleRetry } = useLicenseRefresh();

  const isGrace = state === 'GRACE';

  useEffect(() => {
    if (!isGrace) setDismissed(false);
  }, [isGrace]);

  if (!isGrace || dismissed) return null;

  return (
    <div
      role="status"
      className="flex flex-col gap-2 mx-4 mb-2 px-3 py-2.5 rounded-lg border border-amber-400/30 bg-amber-400/10"
    >
      <div className="flex items-start gap-2">
        <AlertTriangle className="w-3.5 h-3.5 text-amber-300 shrink-0 mt-0.5" aria-hidden="true" />
        <p className="flex-1 text-[11.5px] leading-snug text-amber-200">
          Subscription in grace period
          {daysRemainingInTrial != null
            ? ` — ${daysRemainingInTrial} day${daysRemainingInTrial === 1 ? '' : 's'} left`
            : ''}
          . Resolve payment to avoid losing access.
        </p>
        <button
          onClick={() => setDismissed(true)}
          aria-label="Dismiss grace period notice"
          className="text-amber-300/50 hover:text-amber-200 shrink-0"
        >
          <X className="w-3.5 h-3.5" aria-hidden="true" />
        </button>
      </div>
      <Button
        variant="outline"
        size="sm"
        onClick={handleRetry}
        disabled={isRetrying}
        aria-label="Retry validation now"
        className="h-7 text-[11.5px] w-full border-amber-400/30 text-amber-200 bg-transparent hover:bg-amber-400/10 hover:text-amber-100"
      >
        {isRetrying ? <Loader2 className="w-3.5 h-3.5 animate-spin" aria-hidden="true" /> : 'Retry validation now'}
      </Button>
    </div>
  );
}
