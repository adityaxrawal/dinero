// Doc 30 TASK-LIC-009: backend-observable abuse-pattern detection. Flags for
// manual review only -- never auto-revokes (a false positive, e.g. a
// legitimate user replacing their Mac and forgetting to deactivate first,
// must not lock out a paying customer without recourse). Does not attempt
// binary-level anti-tampering of the desktop client (accepted risk, Doc 01 BR-05).
import type { AuditWriter } from './audit';
import { logAuditEvent } from './audit';

export interface FraudSignal {
  type: 'multiple_device_activations' | 'rapid_activate_deactivate_cycling' | 'signature_tampering_attempts';
  detail: string;
}

const MULTI_DEVICE_WINDOW_MS = 24 * 60 * 60 * 1000; // 24h
const MULTI_DEVICE_THRESHOLD = 3; // distinct devices within the window

const CYCLING_WINDOW_MS = 60 * 60 * 1000; // 1h
const CYCLING_THRESHOLD = 4; // combined activate+deactivate events within the window

interface AuditRow {
  eventType: string;
  deviceFingerprint: string | null;
  createdAt: Date;
}

export function detectFraudSignals(rows: AuditRow[], now: Date = new Date()): FraudSignal[] {
  const signals: FraudSignal[] = [];

  const recentActivations = rows.filter(
    (r) => r.eventType === 'license_activated' && now.getTime() - r.createdAt.getTime() <= MULTI_DEVICE_WINDOW_MS
  );
  const distinctDevices = new Set(recentActivations.map((r) => r.deviceFingerprint).filter(Boolean));
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

  const tamperAttempts = rows.filter((r) => r.eventType === 'jwt_verification_failed');
  if (tamperAttempts.length > 0) {
    signals.push({
      type: 'signature_tampering_attempts',
      detail: `${tamperAttempts.length} failed JWT signature verification attempt(s)`,
    });
  }

  return signals;
}

/// Writes a 'fraud_flag_raised' audit entry per signal -- this is the entire
/// enforcement action. No license/subscription/license_tokens row is ever
/// mutated by this function; a human reviews the flag via the internal
/// admin dashboard and decides (Doc 30 TASK-LIC-009: "provides a documented
/// manual support workflow for force-unbinding a device", not an automated one).
export async function flagForReview(db: AuditWriter, accountId: string, signals: FraudSignal[]): Promise<void> {
  for (const signal of signals) {
    await logAuditEvent(db, {
      accountId,
      eventType: 'fraud_flag_raised',
      payload: { signal_type: signal.type, detail: signal.detail },
    });
  }
}
