/**
 * Navigation footer for the onboarding steps.
 */
import { Loader2, Mail } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { CardFooter } from '@/components/ui/card';

/** Navigation footer for the onboarding steps. */
export default function OnboardingFooter({
  step,
  loading,
  onBack,
  onNext,
  onConnectGmail,
}: {
  step: number;
  loading: boolean;
  onBack: () => void;
  onNext: () => void;
  onConnectGmail: () => void;
}) {
  return (
    <CardFooter className="flex justify-between">
      {step > 1 ? (
        <Button
          variant="outline"
          onClick={onBack}
          disabled={loading}
          aria-label="Go back to previous step"
        >
          Back
        </Button>
      ) : (
        <div aria-hidden="true" />
      )}

      {step === 1 ? (
        <Button onClick={onNext} variant="accent" aria-label="Continue to step 2">
          Continue
        </Button>
      ) : (
        <Button
          onClick={onConnectGmail}
          disabled={loading}
          variant="accent"
          className="gap-2"
          aria-label="I Understand, Continue to Google"
        >
          {loading ? (
            <Loader2 className="w-4 h-4 animate-spin" aria-hidden="true" />
          ) : (
            <Mail className="w-4 h-4" aria-hidden="true" />
          )}
          I Understand, Continue to Google
        </Button>
      )}
    </CardFooter>
  );
}
