/**
 * Renders the licence call-to-action for the current state.
 *
 * Wording and action both come from the shared CTA resolver, so every surface
 * offering a billing action stays consistent.
 */
import { Button } from '@/components/ui/button';
import { getLicenseCta } from '@/lib/licenseCta';
import type { LicenseStatusResponse } from '@/lib/ipc';
import type { PlanId } from './useLicenseActivation';

/** Renders the licence CTA for the current state, from the shared resolver. */
export default function LicenseCtaButton({
  status,
  isCheckingOut,
  onManageBilling,
  onSubscribe,
}: {
  status: LicenseStatusResponse;
  isCheckingOut: boolean;
  onManageBilling: () => void;
  onSubscribe: (planId: PlanId) => void;
}) {
  const cta = getLicenseCta(status.state, status.days_remaining);

  if (cta.action === 'manage_billing') {
    return (
      <Button
        variant="outline"
        className="h-9 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
        onClick={onManageBilling}
      >
        {cta.label}
      </Button>
    );
  }

  if (cta.action === 'update_payment_method') {
    return (
      <Button
        className="h-9 font-semibold bg-amber-600 text-white hover:bg-amber-700"
        onClick={onManageBilling}
      >
        {cta.label}
      </Button>
    );
  }

  return (
    <Button
      className="h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90"
      onClick={() => onSubscribe('desktop_pro_monthly')}
      disabled={isCheckingOut}
    >
      {isCheckingOut ? 'Opening checkout…' : cta.label}
    </Button>
  );
}
