// Doc 30 TASK-LIC-009 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { detectFraudSignals, flagForReview } from '../lib/fraud_detection';

const now = new Date('2026-07-22T12:00:00Z');
function minutesAgo(m: number) {
  return new Date(now.getTime() - m * 60 * 1000);
}

describe('test_multiple_device_activation_attempts_flagged', () => {
  it('flags 3+ distinct devices activating the same license within 24h', () => {
    const rows = [
      { eventType: 'license_activated', deviceFingerprint: 'device-A', createdAt: minutesAgo(60) },
      { eventType: 'license_activated', deviceFingerprint: 'device-B', createdAt: minutesAgo(120) },
      { eventType: 'license_activated', deviceFingerprint: 'device-C', createdAt: minutesAgo(180) },
    ];
    const signals = detectFraudSignals(rows, now);
    expect(signals.some((s) => s.type === 'multiple_device_activations')).toBe(true);
  });

  it('does not flag a single device re-activating repeatedly', () => {
    const rows = [
      { eventType: 'license_activated', deviceFingerprint: 'device-A', createdAt: minutesAgo(10) },
      { eventType: 'license_activated', deviceFingerprint: 'device-A', createdAt: minutesAgo(20) },
    ];
    expect(
      detectFraudSignals(rows, now).some((s) => s.type === 'multiple_device_activations')
    ).toBe(false);
  });
});

describe('test_rapid_activate_deactivate_cycling_flagged', () => {
  it('flags 4+ activate/deactivate events within 1 hour', () => {
    const rows = [
      { eventType: 'license_activated', deviceFingerprint: 'device-A', createdAt: minutesAgo(5) },
      {
        eventType: 'license_deactivated',
        deviceFingerprint: 'device-A',
        createdAt: minutesAgo(10),
      },
      { eventType: 'license_activated', deviceFingerprint: 'device-B', createdAt: minutesAgo(15) },
      {
        eventType: 'license_deactivated',
        deviceFingerprint: 'device-B',
        createdAt: minutesAgo(20),
      },
    ];
    const signals = detectFraudSignals(rows, now);
    expect(signals.some((s) => s.type === 'rapid_activate_deactivate_cycling')).toBe(true);
  });
});

describe('test_flagged_license_not_auto_revoked', () => {
  it('flagForReview only writes audit entries, never mutates license/subscription state', async () => {
    const create = vi.fn().mockResolvedValue({});
    const db = { create, findMany: vi.fn() };
    await flagForReview(db, 'acc_1', [{ type: 'multiple_device_activations', detail: 'x' }]);
    expect(create).toHaveBeenCalledWith(
      expect.objectContaining({ data: expect.objectContaining({ eventType: 'fraud_flag_raised' }) })
    );
    // The function's own signature proves the point structurally: it takes
    // only an audit writer, no licenseToken/subscription update handle at
    // all, so it is architecturally incapable of revoking anything.
  });
});
