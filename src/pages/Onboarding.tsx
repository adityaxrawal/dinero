import { useNavigate } from 'react-router-dom';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';
import GmailConsentScreen from '@/routes/onboarding/GmailConsentScreen';
import HistoricalScanScreen from '@/routes/onboarding/HistoricalScanScreen';
import LicenseActivationScreen from '@/routes/onboarding/LicenseActivationScreen';
import { useOnboardingPreferences } from './onboarding/useOnboardingPreferences';
import { useOnboardingSteps, TOTAL_STEPS } from './onboarding/useOnboardingSteps';
import PreferencesStep from './onboarding/PreferencesStep';
import OnboardingFooter from './onboarding/OnboardingFooter';

export default function Onboarding() {
  const navigate = useNavigate();
  const prefs = useOnboardingPreferences();
  const flow = useOnboardingSteps({ validateLimit: prefs.validateLimit, persist: prefs.persist });
  const { step } = flow;

  const stepLabel = `Step ${step} of ${TOTAL_STEPS}`;

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-lg shadow-2xl">
        <CardHeader>
          <div className="flex items-center justify-between mb-1">
            {/* TASK-FE-004: "Welcome to Dinero" now belongs to the dedicated
                WelcomeScreen shown before this component (OnboardingFlow) —
                this step is the preferences setup that follows it. */}
            <CardTitle className="text-2xl">Set Up Your Preferences</CardTitle>
            <span className="text-xs text-muted-foreground" aria-label={stepLabel}>
              {stepLabel}
            </span>
          </div>
          {/* Step progress bar */}
          <div
            className="w-full h-1.5 bg-secondary/80 rounded-full overflow-hidden"
            role="progressbar"
            aria-valuenow={step}
            aria-valuemin={1}
            aria-valuemax={TOTAL_STEPS}
            aria-label="Onboarding progress"
          >
            <div
              className="h-full rounded-full transition-all duration-500 ease-out"
              style={{ width: `${(step / TOTAL_STEPS) * 100}%`, background: '#064E3B' }}
            />
          </div>
          <CardDescription className="mt-2">
            Let's get your financial command center set up.
          </CardDescription>
        </CardHeader>

        <CardContent>
          {step === 1 && <PreferencesStep prefs={prefs} />}

          {/* TASK-FE-006: the historical scan needs a real, already-connected
              Gmail account_id to scan — it moved here, after Gmail consent
              (step 3), superseding the old months-count dropdown that used
              to sit here and never actually triggered a scan. */}
          {step === 2 && (
            <GmailConsentScreen
              loading={flow.loading}
              oauthError={flow.oauthError}
              showSkip={prefs.statementPref === 'manual'}
              onSkip={flow.handleSkipGmail}
            />
          )}

          {step === 3 && <HistoricalScanScreen onDone={() => flow.setStep(4)} />}

          {step === 4 && <LicenseActivationScreen onContinue={() => navigate('/')} />}
        </CardContent>

        {/* TASK-FE-006/007: steps 3 (HistoricalScanScreen) and 4
            (LicenseActivationScreen) own their own actions inline — a Back
            button there would imply un-connecting Gmail or un-starting the
            trial, neither of which is a real action. */}
        {step < 3 && (
          <OnboardingFooter
            step={step}
            loading={flow.loading}
            onBack={flow.handleBack}
            onNext={flow.handleNext}
            onConnectGmail={flow.handleConnectGmail}
          />
        )}
      </Card>
    </div>
  );
}
