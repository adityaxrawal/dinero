/**
 * Warns that payment failed and access will end when grace elapses.
 *
 * Escalates in urgency as the deadline nears, so a user is not surprised by a
 * lockout that was signalled only quietly.
 */
import { useState, useEffect } from 'react';
import { AlertTriangle, Loader2, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { isGraceUrgent } from './isGraceUrgent';
import { useLicenseStore } from '@/stores/useLicenseStore';
import { useLicenseRefresh } from '@/hooks/useLicenseRefresh';

const PAYMENT_METHOD_PORTAL_URL = 'https://dashboard.razorpay.com/customer-portal';

/** Warns that payment failed and access ends when grace elapses. */
export default function GracePeriodBanner() {
  const state = useLicenseStore((s) => s.state);
  const daysRemainingInTrial = useLicenseStore((s) => s.daysRemainingInTrial);
  const [dismissed, setDismissed] = useState(false);
  const { isRetrying, handleRetry } = useLicenseRefresh();

  const isGrace = state === 'GRACE';
  const urgent = isGraceUrgent(daysRemainingInTrial);

  useEffect(() => {
    if (!isGrace) setDismissed(false);
  }, [isGrace]);

  if (!isGrace || dismissed) return null;

  const palette = urgent
    ? {
        border: 'border-red-500/40',
        bg: 'bg-red-500/10',
        icon: 'text-red-400',
        text: 'text-red-200',
        dismiss: 'text-red-400/50 hover:text-red-200',
      }
    : {
        border: 'border-amber-400/30',
        bg: 'bg-amber-400/10',
        icon: 'text-amber-300',
        text: 'text-amber-200',
        dismiss: 'text-amber-300/50 hover:text-amber-200',
      };

  return (
    <div
      role="status"
      className={`flex flex-col gap-2 mx-4 mb-2 px-3 py-2.5 rounded-lg border ${palette.border} ${palette.bg}`}
    >
      <div className="flex items-start gap-2">
        <AlertTriangle
          className={`w-3.5 h-3.5 ${palette.icon} shrink-0 mt-0.5`}
          aria-hidden="true"
        />
        <p className={`flex-1 text-[11.5px] leading-snug ${palette.text}`}>
          Subscription in grace period
          {daysRemainingInTrial != null
            ? ` — ${daysRemainingInTrial} day${daysRemainingInTrial === 1 ? '' : 's'} left`
            : ''}
          . Resolve payment to avoid losing access.
        </p>
        <button
          onClick={() => setDismissed(true)}
          aria-label="Dismiss grace period notice"
          className={`${palette.dismiss} shrink-0`}
        >
          <X className="w-3.5 h-3.5" aria-hidden="true" />
        </button>
      </div>
      {urgent && (
        <Button
          variant="outline"
          size="sm"
          onClick={async () => {
            const { openUrl } = await import('@tauri-apps/plugin-opener');
            await openUrl(PAYMENT_METHOD_PORTAL_URL);
          }}
          aria-label="Update payment method"
          className="h-7 text-[11.5px] w-full border-red-500/40 text-red-200 bg-transparent hover:bg-red-500/10 hover:text-red-100"
        >
          Update Payment Method
        </Button>
      )}
      <Button
        variant="outline"
        size="sm"
        onClick={handleRetry}
        disabled={isRetrying}
        aria-label="Retry validation now"
        className={`h-7 text-[11.5px] w-full bg-transparent ${urgent ? 'border-red-500/40 text-red-200 hover:bg-red-500/10 hover:text-red-100' : 'border-amber-400/30 text-amber-200 hover:bg-amber-400/10 hover:text-amber-100'}`}
      >
        {isRetrying ? (
          <Loader2 className="w-3.5 h-3.5 animate-spin" aria-hidden="true" />
        ) : (
          'Retry validation now'
        )}
      </Button>
    </div>
  );
}
