/**
 * Decides whether a device and account may begin a new free trial.
 *
 * The abuse this prevents is trial farming -- reinstalling, or signing up with a
 * fresh email, to obtain unlimited free periods. Guarding on either identity
 * alone is insufficient, so both are checked: the account records whether its
 * trial was used, and the device is matched against prior trial_started events.
 *
 * The decision is a pure function over three pre-fetched facts, deliberately
 * separated from the database access that gathers them, so the policy itself is
 * testable in isolation and easy to reason about.
 */
import type { AuditWriter } from './audit';
import { countRecentEvents } from './audit';

export type TrialGuardDecision =
  | { outcome: 'allow_new_trial' }
  | { outcome: 'blocked_email_reused' }
  | { outcome: 'blocked_device_reused' }
  | { outcome: 'recognized_returning_device'; existingSubscriptionStatus: string };

export interface TrialGuardInput {
  accountTrialUsed: boolean;
  deviceBoundSubscriptionStatus: string | null;
  deviceHasPriorTrialStartedEvent: boolean;
}

/**
 * Apply the eligibility policy.
 *
 * Order matters. The returning-device check runs first so that an existing
 * paying customer reinstalling is recognised rather than accused of farming --
 * a device already bound to a non-trial subscription is a legitimate user, and
 * must never be told it has exhausted a trial.
 *
 * Device reuse is then checked before account reuse, since it is the harder
 * signal to fake: a new email is free, a new machine is not.
 */
export function decideTrialEligibility(input: TrialGuardInput): TrialGuardDecision {
  if (input.deviceBoundSubscriptionStatus && input.deviceBoundSubscriptionStatus !== 'trialing') {
    return {
      outcome: 'recognized_returning_device',
      existingSubscriptionStatus: input.deviceBoundSubscriptionStatus,
    };
  }

  if (input.deviceHasPriorTrialStartedEvent) {
    return { outcome: 'blocked_device_reused' };
  }
  if (input.accountTrialUsed) {
    return { outcome: 'blocked_email_reused' };
  }
  return { outcome: 'allow_new_trial' };
}

/**
 * Record a non-trivial guard outcome.
 *
 * Allowed trials are intentionally not logged -- they are the common case, and
 * recording them would bury the blocks that actually warrant attention.
 */
export async function logTrialGuardDecision(
  db: AuditWriter,
  deviceId: string,
  decision: TrialGuardDecision
): Promise<void> {
  const { logAuditEvent } = await import('./audit');
  if (decision.outcome === 'allow_new_trial') return;
  await logAuditEvent(db, {
    eventType:
      decision.outcome === 'recognized_returning_device'
        ? 'trial_guard_recognized_returning_device'
        : 'trial_guard_blocked',
    deviceFingerprint: deviceId,
    payload: { outcome: decision.outcome },
  });
}

/**
 * Whether this device has ever started a trial.
 *
 * Uses an effectively unbounded window, because trial eligibility is a lifetime
 * property: a trial taken years ago still counts. Note this scans the full
 * trial_started history in application code, which is acceptable only while
 * that table stays small.
 *
 * ponytail: full-history scan, add a device index if trial volume grows
 */
export async function deviceHasPriorTrialStartedEvent(
  db: AuditWriter,
  deviceId: string
): Promise<boolean> {
  const count = await countRecentEvents(
    db,
    'trial_started',
    Number.MAX_SAFE_INTEGER,
    (p) => (p as { device_id?: string } | null)?.device_id === deviceId
  );
  return count > 0;
}
