import { useState } from 'react';
import { API } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';

export type PlanId = 'desktop_pro_monthly' | 'desktop_pro_annual';

interface UseLicenseActivationArgs {
  reload: () => Promise<void>;
  setError: (message: string | null) => void;
}

export function useLicenseActivation({ reload, setError }: UseLicenseActivationArgs) {
  const [showActivateForm, setShowActivateForm] = useState(false);
  const [activateEmail, setActivateEmail] = useState('');
  const [activatePaymentId, setActivatePaymentId] = useState('');
  const [activateSignature, setActivateSignature] = useState('');
  const [activateBillingInterval, setActivateBillingInterval] = useState('monthly');
  const [isActivating, setIsActivating] = useState(false);
  const [isCheckingOut, setIsCheckingOut] = useState(false);

  const handleActivateLicense = async () => {
    setIsActivating(true);
    setError(null);
    try {
      await API.licensing.activate(
        activateEmail.trim(),
        activatePaymentId.trim(),
        activateSignature.trim(),
        activateBillingInterval
      );
      setShowActivateForm(false);
      setActivateEmail('');
      setActivatePaymentId('');
      setActivateSignature('');
      await reload();
    } catch (err: unknown) {
      setError(errorMessage(err));
    } finally {
      setIsActivating(false);
    }
  };

  // Doc 30 TASK-BILL-002/010: "Subscribe now"/"Reactivate subscription" CTAs
  // -- opens Razorpay hosted checkout in the system browser (never renders
  // card-entry fields in this app, keeping it entirely out of PCI-DSS
  // scope) and, once the browser redirect confirms payment, activates
  // automatically. Superseded by the manual paste-in form only as a
  // fallback if checkout can't be opened (e.g. no default browser configured).
  const handleSubscribeNow = async (planId: PlanId) => {
    const email = activateEmail.trim();
    if (!email) {
      setShowActivateForm(true);
      return;
    }
    setIsCheckingOut(true);
    setError(null);
    try {
      const { razorpay_payment_id, razorpay_signature } = await API.licensing.startCheckout(
        email,
        planId
      );
      await API.licensing.activate(
        email,
        razorpay_payment_id,
        razorpay_signature,
        planId === 'desktop_pro_annual' ? 'annual' : 'monthly'
      );
      await reload();
    } catch (err: unknown) {
      setError(errorMessage(err));
    } finally {
      setIsCheckingOut(false);
    }
  };

  return {
    showActivateForm,
    setShowActivateForm,
    activateEmail,
    setActivateEmail,
    activatePaymentId,
    setActivatePaymentId,
    activateSignature,
    setActivateSignature,
    activateBillingInterval,
    setActivateBillingInterval,
    isActivating,
    isCheckingOut,
    handleActivateLicense,
    handleSubscribeNow,
  };
}
