import { useState } from 'react';
import WelcomeScreen from './WelcomeScreen';
import NetworkDisclosureScreen from './NetworkDisclosureScreen';
import LegacyOnboardingSteps from '@/pages/Onboarding';

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
    return <NetworkDisclosureScreen onContinue={() => setPhase('setup')} />;
  }
  return <LegacyOnboardingSteps />;
}
