// Doc 30 TASK-BILL-009: extends TASK-LIC-007's device-fingerprint check with
// the combined email+device guard, and the "OS reinstall on the same
// physical Mac" carve-out -- a returning device with an existing paid
// subscription is continuity, not abuse, and must not be blocked.
import type { AuditWriter } from './audit';
import { countRecentEvents } from './audit';

export type TrialGuardDecision =
  | { outcome: 'allow_new_trial' }
  | { outcome: 'blocked_email_reused' }
  | { outcome: 'blocked_device_reused' }
  | { outcome: 'recognized_returning_device'; existingSubscriptionStatus: string };

export interface TrialGuardInput {
  accountTrialUsed: boolean;
  /// The subscription status found on the account bound to this device
  /// fingerprint, if any -- `null` if this device has never been bound to
  /// any account's subscription at all.
  deviceBoundSubscriptionStatus: string | null;
  deviceHasPriorTrialStartedEvent: boolean;
}

export function decideTrialEligibility(input: TrialGuardInput): TrialGuardDecision {
  // Doc 30 TASK-BILL-009: "a returning user on the same Mac is recognized as
  // already-registered and granted a fresh JWT for their existing
  // (non-trial) subscription state -- this is continuity, not abuse."
  if (input.deviceBoundSubscriptionStatus && input.deviceBoundSubscriptionStatus !== 'trialing') {
    return { outcome: 'recognized_returning_device', existingSubscriptionStatus: input.deviceBoundSubscriptionStatus };
  }

  if (input.deviceHasPriorTrialStartedEvent) {
    return { outcome: 'blocked_device_reused' };
  }
  if (input.accountTrialUsed) {
    return { outcome: 'blocked_email_reused' };
  }
  return { outcome: 'allow_new_trial' };
}

/// Doc 30 TASK-BILL-009: "Logs distinguish 'blocked repeat trial attempt'
/// from 'recognized returning device with existing paid subscription.'"
export async function logTrialGuardDecision(db: AuditWriter, deviceId: string, decision: TrialGuardDecision): Promise<void> {
  const { logAuditEvent } = await import('./audit');
  if (decision.outcome === 'allow_new_trial') return; // start-trial.ts itself logs the real trial_started event
  await logAuditEvent(db, {
    eventType: decision.outcome === 'recognized_returning_device' ? 'trial_guard_recognized_returning_device' : 'trial_guard_blocked',
    deviceFingerprint: deviceId,
    payload: { outcome: decision.outcome },
  });
}

/// Helper for callers that only have raw audit-log access (matches
/// start-trial.ts's existing device-history check style).
export async function deviceHasPriorTrialStartedEvent(db: AuditWriter, deviceId: string): Promise<boolean> {
  const count = await countRecentEvents(db, 'trial_started', Number.MAX_SAFE_INTEGER, (p) => (p as { device_id?: string } | null)?.device_id === deviceId);
  return count > 0;
}
