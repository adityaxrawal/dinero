import { useState } from 'react';
import { API } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';
import { OUTBOUND_CHANNEL_DISCLOSURE, BETA_PROGRAM_DISCLOSURE } from '@/constants/privacy';
import { mapOauthError } from '@/routes/onboarding/mapOauthError';

export const TOTAL_STEPS = 4;

// TASK-AUTH-003 (Document 18 §4.21a): `disclosure_text` must be the exact
// verbatim text shown to the user at consent time, not a paraphrase of what
// they did — this is that verbatim text, assembled from the same constants
// the consent screen itself renders from.
const GMAIL_CONSENT_DISCLOSURE_TEXT = [
  'Requested Gmail scope: https://www.googleapis.com/auth/gmail.readonly',
  ...BETA_PROGRAM_DISCLOSURE,
  ...OUTBOUND_CHANNEL_DISCLOSURE,
].join(' | ');

/** Consent logging must never block onboarding. */
async function recordConsent(kind: string, text: string) {
  try {
    await API.privacy.recordConsentEvent(kind, text);
  } catch (consentErr) {
    console.error(`Failed to record ${kind} consent event:`, consentErr);
  }
}

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

  const handleNext = () => {
    if (step === 1 && !validateLimit()) return;
    setStep((s) => Math.min(s + 1, TOTAL_STEPS));
  };

  const handleBack = () => {
    setOauthError(null);
    setStep((s) => Math.max(s - 1, 1));
  };

  const handleConnectGmail = async () => {
    setLoading(true);
    setOauthError(null);
    try {
      // TASK-AUTH-003 (Document 30): "on consent-screen acknowledgment,
      // insert a gmail_oauth_consent row" — recorded at the moment of
      // acknowledgment, before the OAuth round-trip even starts (not tied to
      // whether it later succeeds).
      await recordConsent('gmail_oauth_consent', GMAIL_CONSENT_DISCLOSURE_TEXT);
      // Must succeed before marking onboarded.
      await API.auth.startGoogle();
      await persist();
      // TASK-FE-006: advance to the historical-scan step instead of finishing
      // onboarding immediately — a connected account is exactly what that
      // step needs to actually trigger a scan.
      setStep(3);
    } catch (e: unknown) {
      setOauthError(mapOauthError(errorMessage(e)));
    } finally {
      setLoading(false);
    }
  };

  // G2 fix: statement-only users previously had no way to finish onboarding
  // without connecting Gmail — this persists the same onboarding state
  // without the OAuth step, for users who selected "Manual" in step 1.
  const handleSkipGmail = async () => {
    setLoading(true);
    try {
      await recordConsent('onboarding_disclosure', OUTBOUND_CHANNEL_DISCLOSURE.join(' | '));
      await persist();
      // TASK-FE-007: the trial-confirmation screen is universal (not
      // Gmail-specific) — manual/statement-only users skip the historical
      // scan (no account to scan) but still see it.
      setStep(4);
    } finally {
      setLoading(false);
    }
  };

  return { step, setStep, loading, oauthError, handleNext, handleBack, handleConnectGmail, handleSkipGmail };
}
