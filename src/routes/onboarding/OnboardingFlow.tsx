import { useState } from 'react';
import WelcomeScreen from './WelcomeScreen';
import NetworkDisclosureScreen from './NetworkDisclosureScreen';
import LegacyOnboardingSteps from '@/pages/Onboarding';
import { API } from '@/lib/ipc';

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
  const [phase, setPhase] = useState<Phase>('welcome');

  if (phase === 'welcome') {
    return <WelcomeScreen onContinue={() => setPhase('disclosure')} />;
  }
  if (phase === 'disclosure') {
    return (
      <NetworkDisclosureScreen
        onContinue={() => {
          // Acknowledgement is recorded backend-side as a durable consent
          // event. Deliberately not awaited -- advancing the user must not
          // depend on the write succeeding.
          void API.privacy.recordConsentEvent(
            'network_disclosure_acknowledged',
            'User acknowledged the onboarding network-communication disclosure screen.'
          );
          setPhase('setup');
        }}
      />
    );
  }
  return <LegacyOnboardingSteps />;
}
