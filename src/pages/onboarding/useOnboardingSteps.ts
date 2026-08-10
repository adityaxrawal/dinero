/**
 * Step sequencing for onboarding, including which steps apply.
 */
import { useState } from 'react';
import { API } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';
import { OUTBOUND_CHANNEL_DISCLOSURE, BETA_PROGRAM_DISCLOSURE } from '@/constants/privacy';
import { mapOauthError } from '@/routes/onboarding/mapOauthError';

export const TOTAL_STEPS = 4;

const GMAIL_CONSENT_DISCLOSURE_TEXT = [
  'Requested Gmail scope: https://www.googleapis.com/auth/gmail.readonly',
  ...BETA_PROGRAM_DISCLOSURE,
  ...OUTBOUND_CHANNEL_DISCLOSURE,
].join(' | ');

/**
 * Records a consent event as the user advances past a disclosure.
 *
 * Deliberately not awaited by callers -- advancing the user must not depend on
 * the write succeeding.
 */
async function recordConsent(kind: string, text: string) {
  try {
    await API.privacy.recordConsentEvent(kind, text);
  } catch (consentErr) {
    console.error(`Failed to record ${kind} consent event:`, consentErr);
  }
}

/** Step sequencing for onboarding, including which steps apply. */
export function useOnboardingSteps({
  validateLimit,
  persist,
}: {
  validateLimit: () => boolean;
  persist: () => Promise<void>;
}) {
  const [step, setStep] = useState(1);
  const [loading, setLoading] = useState(false);
  const [oauthError, setOauthError] = useState<string | null>(null);

  /** Advances to the next step. */
  const handleNext = () => {
    if (step === 1 && !validateLimit()) return;
    setStep((s) => Math.min(s + 1, TOTAL_STEPS));
  };

  /** Returns to the previous step. */
  const handleBack = () => {
    setOauthError(null);
    setStep((s) => Math.max(s - 1, 1));
  };

  /** Starts the Gmail OAuth flow. */
  const handleConnectGmail = async () => {
    setLoading(true);
    setOauthError(null);
    try {
      await recordConsent('gmail_oauth_consent', GMAIL_CONSENT_DISCLOSURE_TEXT);
      await API.auth.startGoogle();
      await persist();
      setStep(3);
    } catch (e: unknown) {
      setOauthError(mapOauthError(errorMessage(e)));
    } finally {
      setLoading(false);
    }
  };

  /** Skips Gmail, leaving the app on manual statement upload alone. */
  const handleSkipGmail = async () => {
    setLoading(true);
    try {
      await recordConsent('onboarding_disclosure', OUTBOUND_CHANNEL_DISCLOSURE.join(' | '));
      await persist();
      setStep(4);
    } finally {
      setLoading(false);
    }
  };

  return { step, setStep, loading, oauthError, handleNext, handleBack, handleConnectGmail, handleSkipGmail };
}
