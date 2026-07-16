import { useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { CheckCircle2 } from 'lucide-react';
import { useLicenseStore } from '@/stores/useLicenseStore';

interface LicenseActivationScreenProps {
  onContinue: () => void;
}

function formatDate(iso: string | null): string | null {
  if (!iso) return null;
  try {
    return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'long', day: 'numeric' });
  } catch {
    return null;
  }
}

/**
 * TASK-FE-007 (Doc 30): the 14-day trial auto-starts with no user action
 * needed — it's derived server-side from `local_profile.created_at`
 * (`licensing::gate::trial_days_remaining`), not a transition this screen
 * triggers. This screen just confirms it via `useLicenseStore`.
 *
 * Doc30's "Already have a license key?" secondary path doesn't match the
 * real backend: `license_activate` (Document 19 §14.2) takes Razorpay
 * payment-confirmation fields, not a license key, and no Razorpay checkout
 * flow exists yet (Area 12/BILL is unbuilt). That activation form already
 * exists in Settings → License (against the same real IPC contract) — this
 * screen points there rather than duplicating a form with nothing real to
 * submit until checkout exists. Revisit when Area 12 ships.
 */
export default function LicenseActivationScreen({ onContinue }: LicenseActivationScreenProps) {
  const { state, hydrated, expiryDate, hydrate } = useLicenseStore();

  useEffect(() => {
    hydrate();
  }, [hydrate]);

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
          <p className="text-sm text-muted-foreground mt-2">All paid features are unlocked on this Mac.</p>
        </div>
      ) : (
        <div>
          <h3 className="text-lg font-medium">Your 14-day free trial has started</h3>
          <p className="text-sm text-muted-foreground mt-2">
            {trialEndsOn ? (
              <>Trial ends on <span className="font-medium text-foreground">{trialEndsOn}</span>. No credit card required.</>
            ) : (
              'No credit card required.'
            )}
          </p>
        </div>
      )}

      {!alreadyPaid && (
        <p className="text-xs text-muted-foreground bg-secondary/50 rounded-md p-3">
          Already have a subscription? You can activate it from{' '}
          <span className="font-medium text-foreground">Settings → License</span> once you've completed checkout.
        </p>
      )}

      <Button onClick={onContinue} variant="accent" aria-label="Continue to the dashboard">
        Continue to Dashboard
      </Button>
    </div>
  );
}
