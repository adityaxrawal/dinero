import { useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { CheckCircle2 } from 'lucide-react';
import { useLicenseStore } from '@/stores/useLicenseStore';

/**
 * Onboarding step covering trial or subscription status.
 *
 * Adapts to what the user already has: someone with an active or grace-period
 * subscription is confirmed rather than sold to, while everyone else sees their
 * trial terms. Hydrates the license store on mount because onboarding can run
 * before the app-wide hydration has completed.
 */
interface LicenseActivationScreenProps {
  onContinue: () => void;
}

/**
 * Format an ISO date for display, or null if it is missing or unparseable.
 *
 * Returning null rather than a placeholder lets the caller omit the whole
 * sentence, instead of rendering one with "Invalid Date" inside it.
 */
function formatDate(iso: string | null): string | null {
  if (!iso) return null;
  const parsed = new Date(iso);
  if (Number.isNaN(parsed.getTime())) return null;
  return parsed.toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  });
}

/** Trial or subscription status, adapted to what the user already holds. */
export default function LicenseActivationScreen({ onContinue }: LicenseActivationScreenProps) {
  const { state, hydrated, expiryDate, hydrate } = useLicenseStore();

  useEffect(() => {
    hydrate();
  }, [hydrate]);

  // GRACE counts as paid here: the subscription exists and payment merely needs
  // attention, so presenting a purchase prompt would be wrong.
  const alreadyPaid = state === 'ACTIVE' || state === 'GRACE';
  const trialEndsOn = formatDate(expiryDate);

  return (
    <div className="space-y-6 text-center animate-in fade-in slide-in-from-bottom-4">
      <div className="mx-auto w-12 h-12 rounded-full bg-emerald-500/10 flex items-center justify-center">
        <CheckCircle2 className="w-6 h-6 text-emerald-600" aria-hidden="true" />
      </div>

      {!hydrated ? (
        <p className="text-sm text-muted-foreground">Checking your license status…</p>
      ) : alreadyPaid ? (
        <div>
          <h3 className="text-lg font-medium">You already have an active subscription</h3>
          <p className="text-sm text-muted-foreground mt-2">
            All paid features are unlocked on this Mac.
          </p>
        </div>
      ) : (
        <div>
          <h3 className="text-lg font-medium">Your 14-day free trial has started</h3>
          <p className="text-sm text-muted-foreground mt-2">
            {trialEndsOn ? (
              <>
                Trial ends on <span className="font-medium text-foreground">{trialEndsOn}</span>. No
                credit card required.
              </>
            ) : (
              'No credit card required.'
            )}
          </p>
        </div>
      )}

      {!alreadyPaid && (
        <p className="text-xs text-muted-foreground bg-secondary/50 rounded-md p-3">
          Already have a subscription? You can activate it from{' '}
          <span className="font-medium text-foreground">Settings → License</span> once you've
          completed checkout.
        </p>
      )}

      <Button onClick={onContinue} variant="accent" aria-label="Continue to the dashboard">
        Continue to Dashboard
      </Button>
    </div>
  );
}
