import { Button } from '@/components/ui/button';
import type { useLicenseActivation } from './useLicenseActivation';

const FIELD_CLASS =
  'w-full px-3 py-2 rounded-lg border text-[13px] font-medium bg-[#F8E7C9]/50 border-[#064E3B]/20 text-[#064E3B] focus:border-[#064E3B] focus:ring-1 focus:ring-[#064E3B]';
const INPUT_CLASS = `${FIELD_CLASS} placeholder:text-[#064E3B]/40`;

/** Fallback for when the hosted Razorpay checkout can't be opened — takes the
 *  confirmation Razorpay already issued. Never collects card details. */
export default function ManualActivationForm({
  activation,
}: {
  activation: ReturnType<typeof useLicenseActivation>;
}) {
  const {
    activateEmail,
    setActivateEmail,
    activatePaymentId,
    setActivatePaymentId,
    activateSignature,
    setActivateSignature,
    activateBillingInterval,
    setActivateBillingInterval,
    isActivating,
    handleActivateLicense,
  } = activation;

  const incomplete =
    !activateEmail.trim() || !activatePaymentId.trim() || !activateSignature.trim();

  return (
    <div className="mt-8 pt-6 border-t border-[#064E3B]/10 space-y-4">
      <p className="text-[13px] font-medium text-[#064E3B]/70">
        Enter the payment confirmation details from your Razorpay checkout.
      </p>
      <div className="grid gap-4 max-w-sm">
        <input
          className={INPUT_CLASS}
          placeholder="Email"
          value={activateEmail}
          onChange={(e) => setActivateEmail(e.target.value)}
        />
        <input
          className={INPUT_CLASS}
          placeholder="Payment ID"
          value={activatePaymentId}
          onChange={(e) => setActivatePaymentId(e.target.value)}
        />
        <input
          className={INPUT_CLASS}
          placeholder="Signature"
          value={activateSignature}
          onChange={(e) => setActivateSignature(e.target.value)}
        />
        <select
          className={FIELD_CLASS}
          value={activateBillingInterval}
          onChange={(e) => setActivateBillingInterval(e.target.value)}
        >
          <option value="monthly">Monthly</option>
          <option value="yearly">Yearly</option>
        </select>
        <Button
          className="h-9 font-semibold bg-[#064E3B] text-[#F8E7C9] hover:bg-[#064E3B]/90"
          onClick={handleActivateLicense}
          disabled={isActivating || incomplete}
        >
          {isActivating ? 'Activating…' : 'Confirm Activation'}
        </Button>
      </div>
    </div>
  );
}
