// Review P4: the preferences save also marks onboarding complete in the
// database, so a failed save must keep the user on the step with a reason.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useOnboardingSteps } from '@/pages/onboarding/useOnboardingSteps';

const startGoogle = vi.fn();
vi.mock('@/lib/ipc', () => ({
  API: {
    auth: { startGoogle: () => startGoogle() },
    privacy: { recordConsentEvent: vi.fn().mockResolvedValue(undefined) },
  },
}));

function setup(persist: () => Promise<void>) {
  const { result } = renderHook(() => useOnboardingSteps({ validateLimit: () => true, persist }));
  act(() => result.current.setStep(2));
  return result;
}

describe('useOnboardingSteps save failures', () => {
  beforeEach(() => startGoogle.mockReset().mockResolvedValue(undefined));

  it('skip Gmail: a failed save stays on the step and says why', async () => {
    const result = setup(() => Promise.reject(new Error('disk full')));
    await act(() => result.current.handleSkipGmail());
    expect(result.current.step).toBe(2);
    expect(result.current.oauthError).toContain('Could not save your preferences: disk full');
  });

  it('connect Gmail: a failed save is reported as a save failure, not an OAuth one', async () => {
    const result = setup(() => Promise.reject(new Error('disk full')));
    await act(() => result.current.handleConnectGmail());
    expect(result.current.step).toBe(2);
    expect(result.current.oauthError).toContain('Could not save your preferences');
  });

  it('advances once the save succeeds', async () => {
    const result = setup(() => Promise.resolve());
    await act(() => result.current.handleSkipGmail());
    expect(result.current.step).toBe(4);
    expect(result.current.oauthError).toBeNull();
  });
});
