// Doc 30 TASK-BILL-009 acceptance criteria.
import { describe, it, expect } from 'vitest';
import { decideTrialEligibility } from '../lib/trial_guard';

describe('test_trial_blocked_for_reused_email', () => {
  it('blocks when the account has already used its trial and the device is unknown', () => {
    const decision = decideTrialEligibility({
      accountTrialUsed: true,
      deviceBoundSubscriptionStatus: null,
      deviceHasPriorTrialStartedEvent: false,
    });
    expect(decision.outcome).toBe('blocked_email_reused');
  });
});

describe('test_trial_blocked_for_reused_device_fingerprint', () => {
  it('blocks when this device already started a trial, regardless of email', () => {
    const decision = decideTrialEligibility({
      accountTrialUsed: false,
      deviceBoundSubscriptionStatus: null,
      deviceHasPriorTrialStartedEvent: true,
    });
    expect(decision.outcome).toBe('blocked_device_reused');
  });
});

describe('test_os_reinstall_recognized_not_blocked_as_abuse', () => {
  it('recognizes a device already bound to an active (non-trial) subscription as returning, not abuse', () => {
    const decision = decideTrialEligibility({
      accountTrialUsed: true,
      deviceBoundSubscriptionStatus: 'active',
      deviceHasPriorTrialStartedEvent: true,
    });
    expect(decision).toEqual({
      outcome: 'recognized_returning_device',
      existingSubscriptionStatus: 'active',
    });
  });

  it('still treats a device bound only to a trialing subscription as an ordinary device-reuse block', () => {
    // A "trialing" binding is not proof of a completed purchase -- someone
    // reinstalling mid-trial gets the ordinary device-reuse rejection, not
    // the returning-customer carve-out.
    const decision = decideTrialEligibility({
      accountTrialUsed: true,
      deviceBoundSubscriptionStatus: 'trialing',
      deviceHasPriorTrialStartedEvent: true,
    });
    expect(decision.outcome).toBe('blocked_device_reused');
  });

  it('allows a genuinely new device with a genuinely new email', () => {
    const decision = decideTrialEligibility({
      accountTrialUsed: false,
      deviceBoundSubscriptionStatus: null,
      deviceHasPriorTrialStartedEvent: false,
    });
    expect(decision.outcome).toBe('allow_new_trial');
  });
});
