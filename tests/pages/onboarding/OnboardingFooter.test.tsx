import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import OnboardingFooter from '@/pages/onboarding/OnboardingFooter';

const onBack = vi.fn();
const onNext = vi.fn();
const onConnectGmail = vi.fn();

function renderFooter(over: Record<string, unknown> = {}) {
  return render(
    <OnboardingFooter
      step={1}
      loading={false}
      onBack={onBack}
      onNext={onNext}
      onConnectGmail={onConnectGmail}
      {...over}
    />
  );
}

beforeEach(() => vi.clearAllMocks());

describe('OnboardingFooter', () => {
  it('offers no way back from the first step', () => {
    renderFooter({ step: 1 });

    expect(screen.queryByRole('button', { name: 'Go back to previous step' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Continue to step 2' })).toBeInTheDocument();
  });

  it('swaps Continue for the Google hand-off after the first step', () => {
    renderFooter({ step: 2 });

    expect(screen.getByRole('button', { name: 'Go back to previous step' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Continue to step 2' })).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'I Understand, Continue to Google' })
    ).toBeInTheDocument();
  });

  it('routes each button to its own handler', () => {
    renderFooter({ step: 2 });

    fireEvent.click(screen.getByRole('button', { name: 'Go back to previous step' }));
    expect(onBack).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole('button', { name: 'I Understand, Continue to Google' }));
    expect(onConnectGmail).toHaveBeenCalledTimes(1);
    expect(onNext).not.toHaveBeenCalled();
  });

  it('advances rather than connecting on step 1', () => {
    renderFooter({ step: 1 });

    fireEvent.click(screen.getByRole('button', { name: 'Continue to step 2' }));
    expect(onNext).toHaveBeenCalledTimes(1);
    expect(onConnectGmail).not.toHaveBeenCalled();
  });

  it('blocks navigation while the OAuth round trip is open', () => {
    renderFooter({ step: 2, loading: true });

    expect(screen.getByRole('button', { name: 'Go back to previous step' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'I Understand, Continue to Google' })).toBeDisabled();
  });

  it('leaves step 1 Continue enabled while loading, since nothing is in flight yet', () => {
    renderFooter({ step: 1, loading: true });

    expect(screen.getByRole('button', { name: 'Continue to step 2' })).toBeEnabled();
  });
});
