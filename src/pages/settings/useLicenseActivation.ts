/**
 * Drives licence activation and its pending state.
 */
import { useState } from 'react';
import { API } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';

export type PlanId = 'desktop_pro_monthly' | 'desktop_pro_annual';

interface UseLicenseActivationArgs {
  reload: () => Promise<void>;
  setError: (message: string | null) => void;
}

/** Drives licence activation and its pending state. */
export function useLicenseActivation({ reload, setError }: UseLicenseActivationArgs) {
  const [showActivateForm, setShowActivateForm] = useState(false);
  const [activateEmail, setActivateEmail] = useState('');
  const [activatePaymentId, setActivatePaymentId] = useState('');
  const [activateSignature, setActivateSignature] = useState('');
  const [activateBillingInterval, setActivateBillingInterval] = useState('monthly');
  const [isActivating, setIsActivating] = useState(false);
  const [isCheckingOut, setIsCheckingOut] = useState(false);

  /** Activates using a manually entered key. */
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

  /** Starts the purchase flow. */
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
