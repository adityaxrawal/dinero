import { useState } from 'react';
import WelcomeScreen from './WelcomeScreen';
import NetworkDisclosureScreen from './NetworkDisclosureScreen';
import LegacyOnboardingSteps from '@/pages/Onboarding';
import { API } from '@/lib/ipc';

type Phase = 'welcome' | 'disclosure' | 'setup';

/**
 * TASK-FE-004 (Doc 30): the onboarding flow's entry orchestrator — Welcome,
 * then the Network Disclosure gate (both built by this task), then the rest
 * of the flow (preferences / Gmail consent / historical scan / license
 * activation). `LegacyOnboardingSteps` is `src/pages/Onboarding.tsx`'s
 * existing step wizard, temporarily still one component; TASK-FE-005/006/007
 * extract its remaining steps (Gmail consent, historical scan, license
 * activation) into their own named screens one task at a time, same pattern
 * as this task's two extractions.
 */
export default function OnboardingFlow() {
  const [phase, setPhase] = useState<Phase>('welcome');

  if (phase === 'welcome') {
    return <WelcomeScreen onContinue={() => setPhase('disclosure')} />;
  }
  if (phase === 'disclosure') {
    return (
      <NetworkDisclosureScreen
        onContinue={() => {
          // TASK-DESK-002: records that the user has actually seen the
          // network disclosure, via the existing generic consent-event
          // recorder (Doc 25 §4.2/§4.4) rather than a new dedicated command
          // -- the Rust side gates native-notification permission requests
          // on this event existing, so it's never requested proactively
          // before this screen has been shown. Best-effort: onboarding must
          // still advance even if this write fails.
          void API.privacy.recordConsentEvent(
            'network_disclosure_acknowledged',
            'User acknowledged the onboarding network-communication disclosure screen (Document 01 §10.4).'
          );
          setPhase('setup');
        }}
      />
    );
  }
  return <LegacyOnboardingSteps />;
}
