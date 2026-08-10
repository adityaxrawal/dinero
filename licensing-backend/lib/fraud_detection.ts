/**
 * Heuristics that flag licensing abuse for human review.
 *
 * Three patterns are looked for, each corresponding to a distinct abuse shape:
 * one license spreading across many machines (sharing), rapid activate/
 * deactivate churn (rotating a single seat between users), and failed signature
 * verifications (an attempt to forge or tamper with a token).
 *
 * Nothing here blocks a user. Signals are recorded for review, because every
 * threshold has innocent explanations -- a genuine hardware upgrade, a
 * reinstall, a machine restored from backup -- and automatically locking out a
 * paying customer on a heuristic is a worse failure than a missed detection.
 */
import type { AuditWriter } from './audit';
import { logAuditEvent } from './audit';

export interface FraudSignal {
  type:
    | 'multiple_device_activations'
    | 'rapid_activate_deactivate_cycling'
    | 'signature_tampering_attempts';
  detail: string;
}

// Sharing: several distinct machines activating the same license in a day.
const MULTI_DEVICE_WINDOW_MS = 24 * 60 * 60 * 1000;
const MULTI_DEVICE_THRESHOLD = 3;

// Seat rotation: repeated activate/deactivate churn within the hour.
const CYCLING_WINDOW_MS = 60 * 60 * 1000;
const CYCLING_THRESHOLD = 4;

interface AuditRow {
  eventType: string;
  deviceFingerprint: string | null;
  createdAt: Date;
}

/**
 * Evaluate all three heuristics over an account's audit rows.
 *
 * Pure, and takes `now` as a parameter rather than reading the clock, so the
 * time windows can be tested deterministically. Returns every signal that
 * fired -- these are independent, and more than one may apply at once.
 */
export function detectFraudSignals(rows: AuditRow[], now: Date = new Date()): FraudSignal[] {
  const signals: FraudSignal[] = [];

  const recentActivations = rows.filter(
    (r) =>
      r.eventType === 'license_activated' &&
      now.getTime() - r.createdAt.getTime() <= MULTI_DEVICE_WINDOW_MS
  );
  // Counted by distinct fingerprint, not by event: one machine reactivating
  // repeatedly is churn, which the next heuristic covers, not sharing.
  const distinctDevices = new Set(
    recentActivations.map((r) => r.deviceFingerprint).filter(Boolean)
  );
  if (distinctDevices.size >= MULTI_DEVICE_THRESHOLD) {
    signals.push({
      type: 'multiple_device_activations',
      detail: `${distinctDevices.size} distinct devices activated this license within 24h`,
    });
  }

  const recentCycling = rows.filter(
    (r) =>
      (r.eventType === 'license_activated' || r.eventType === 'license_deactivated') &&
      now.getTime() - r.createdAt.getTime() <= CYCLING_WINDOW_MS
  );
  if (recentCycling.length >= CYCLING_THRESHOLD) {
    signals.push({
      type: 'rapid_activate_deactivate_cycling',
      detail: `${recentCycling.length} activate/deactivate events within 1h`,
    });
  }

  // No window and a threshold of one: a failed signature check has no benign
  // explanation, so even a single occurrence is worth surfacing.
  const tamperAttempts = rows.filter((r) => r.eventType === 'jwt_verification_failed');
  if (tamperAttempts.length > 0) {
    signals.push({
      type: 'signature_tampering_attempts',
      detail: `${tamperAttempts.length} failed JWT signature verification attempt(s)`,
    });
  }

  return signals;
}

/**
 * Persist each signal as its own audit entry.
 *
 * Written to the same audit log the signals were derived from, so a reviewer
 * sees the flag in sequence with the events that triggered it.
 */
export async function flagForReview(
  db: AuditWriter,
  accountId: string,
  signals: FraudSignal[]
): Promise<void> {
  for (const signal of signals) {
    await logAuditEvent(db, {
      accountId,
      eventType: 'fraud_flag_raised',
      payload: { signal_type: signal.type, detail: signal.detail },
    });
  }
}
