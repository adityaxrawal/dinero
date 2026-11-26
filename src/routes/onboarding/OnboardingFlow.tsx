import { useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import WelcomeScreen from './WelcomeScreen';
import NetworkDisclosureScreen from './NetworkDisclosureScreen';
import SetupSteps from './SetupSteps';
import { API } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';

/**
 * Three-phase onboarding: welcome, network disclosure, then account setup.
 *
 * Phase is local state with no route of its own, so the flow cannot be entered
 * halfway through or navigated back into by URL -- the disclosure must be seen
 * before setup begins.
 */
type Phase = 'welcome' | 'disclosure' | 'setup';

/** Three-phase onboarding: welcome, disclosure, then setup. */
export default function OnboardingFlow() {
  const navigate = useNavigate();
  // Set when an already-onboarded install still owes the disclosure.
  const consentOnly = Boolean(
    (useLocation().state as { consentOnly?: boolean } | null)?.consentOnly
  );
  const [phase, setPhase] = useState<Phase>(consentOnly ? 'disclosure' : 'welcome');
  const [consentError, setConsentError] = useState<string | null>(null);

  if (phase === 'welcome') {
    return <WelcomeScreen onContinue={() => setPhase('disclosure')} />;
  }
  if (phase === 'disclosure') {
    return (
      <NetworkDisclosureScreen
        error={consentError}
        onContinue={async () => {
          // The backend's background network calls wait for this record
          // (review P4), so the user advances only once it is stored.
          try {
            await API.privacy.recordConsentEvent(
              'network_disclosure_acknowledged',
              'User acknowledged the onboarding network-communication disclosure screen.'
            );
          } catch (err) {
            setConsentError(`Could not save your acknowledgement: ${errorMessage(err)}`);
            return;
          }
          if (consentOnly) navigate('/', { replace: true });
          else setPhase('setup');
        }}
      />
    );
  }
  return <SetupSteps />;
}
