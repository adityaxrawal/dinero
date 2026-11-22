// Doc 30 TASK-LIC-007 acceptance criteria (DB shape extended for
// TASK-BILL-009's returning-device recognition, see tests/trial_guard.test.ts
// for the pure decision-logic tests).
import { describe, it, expect, vi } from 'vitest';
import { startTrial, type StartTrialDb } from '../api/license/start-trial';
import { computeConversionFunnel } from '../lib/trial_metrics';
import { TEST_PRIVATE_KEY_PEM } from './testKeys';

function makeDb(opts: {
  existingAccount?: Record<string, unknown> | null;
  existingBinding?: Record<string, unknown> | null;
  boundSubscription?: Record<string, unknown> | null;
  priorDeviceTrialAuditRows?: unknown[];
}): StartTrialDb {
  const auditRows = opts.priorDeviceTrialAuditRows ?? [];
  return {
    account: {
      findUnique: vi.fn().mockResolvedValue(opts.existingAccount ?? null),
      create: vi
        .fn()
        .mockResolvedValue({ id: 'acc_new', email: 'new@example.com', trialUsed: false }),
      update: vi.fn().mockResolvedValue({}),
    } as unknown as StartTrialDb['account'],
    subscription: {
      create: vi.fn().mockResolvedValue({}),
      findFirst: vi.fn().mockResolvedValue(opts.boundSubscription ?? null),
    } as unknown as StartTrialDb['subscription'],
    licenseToken: {
      create: vi.fn().mockResolvedValue({}),
      findUnique: vi.fn().mockResolvedValue(opts.existingBinding ?? null),
    } as unknown as StartTrialDb['licenseToken'],
    licensingAuditLog: {
      create: vi.fn().mockImplementation(({ data }) => {
        auditRows.push({ eventType: data.eventType, payload: data.payload, createdAt: new Date() });
        return Promise.resolve({});
      }),
      findMany: vi
        .fn()
        .mockImplementation(({ where }: { where: { eventType: string } }) =>
          Promise.resolve(
            auditRows.filter((r) => (r as { eventType: string }).eventType === where.eventType)
          )
        ),
    } as unknown as StartTrialDb['licensingAuditLog'],
  };
}

describe('test_trial_one_per_hardware_uuid', () => {
  it('rejects a second trial start on the same device', async () => {
    const db = makeDb({
      existingAccount: null,
      priorDeviceTrialAuditRows: [
        { eventType: 'trial_started', payload: { device_id: 'device-A' }, createdAt: new Date() },
      ],
    });
    await expect(
      startTrial(db, { email: 'new@example.com', device_id: 'device-A' }, TEST_PRIVATE_KEY_PEM)
    ).rejects.toMatchObject({
      code: 'VALIDATION_ERROR',
    });
  });

  it('allows the first trial on a fresh device', async () => {
    const db = makeDb({ existingAccount: null });
    await expect(
      startTrial(db, { email: 'new@example.com', device_id: 'device-A' }, TEST_PRIVATE_KEY_PEM)
    ).resolves.toMatchObject({
      status: 'trial_started',
    });
  });
});

describe('test_trial_expires_after_14_days', () => {
  it('sets trial_ends_at exactly 14 days from now', async () => {
    const db = makeDb({ existingAccount: null });
    const before = Date.now();
    const result = await startTrial(
      db,
      { email: 'new@example.com', device_id: 'device-A' },
      TEST_PRIVATE_KEY_PEM
    );
    if (result.status !== 'trial_started') throw new Error('expected trial_started');
    const expiryMs = new Date(result.trial_ends_at).getTime();
    const expectedMs = before + 14 * 24 * 60 * 60 * 1000;
    expect(Math.abs(expiryMs - expectedMs)).toBeLessThan(5000);
  });
});

describe('test_conversion_funnel_events_tracked', () => {
  it('counts trial_started events written by startTrial', async () => {
    const db = makeDb({ existingAccount: null });
    await startTrial(db, { email: 'a@example.com', device_id: 'device-A' }, TEST_PRIVATE_KEY_PEM);
    const summary = await computeConversionFunnel(db.licensingAuditLog, 30);
    expect(summary.trials_started).toBe(1);
    expect(summary.converted).toBe(0);
  });
});

describe('test_os_reinstall_recognized_not_blocked_as_abuse (integration)', () => {
  it('re-issues a JWT for the existing subscription instead of starting a second trial', async () => {
    const db = makeDb({
      existingAccount: { id: 'acc_1', email: 'user@example.com', trialUsed: true },
      existingBinding: { accountId: 'acc_1' },
      boundSubscription: {
        planId: 'desktop_pro_annual',
        billingInterval: 'annual',
        status: 'active',
      },
    });
    const result = await startTrial(
      db,
      { email: 'user@example.com', device_id: 'device-A' },
      TEST_PRIVATE_KEY_PEM
    );
    expect(result).toMatchObject({
      status: 'existing_subscription_recognized',
      plan: 'desktop_pro_annual',
    });
  });
});

// Review M15: the desktop app registers its trial at first launch by device
// only; a reinstall or data reset gets the same trial back, and a lapsed
// trial is signed as LOCKED.
describe('device-only trials', () => {
  it('starts a trial without an email, using a placeholder .invalid address', async () => {
    const db = makeDb({});
    const result = await startTrial(db, { device_id: 'device-Z' }, TEST_PRIVATE_KEY_PEM);
    expect(result.status).toBe('trial_started');
    const created = (db.account.create as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(created.data.email).toMatch(/^device-[0-9a-f]{24}@trial\.invalid$/);
  });

  it('a reset on a device bound to a running trial resumes it with the original end date', async () => {
    const endsAt = new Date(Date.now() + 5 * 24 * 60 * 60 * 1000);
    const db = makeDb({
      existingBinding: { accountId: 'acc_trial' },
      boundSubscription: {
        status: 'trialing',
        planId: 'desktop_pro_monthly',
        billingInterval: 'monthly',
        currentPeriodEnd: endsAt,
      },
      priorDeviceTrialAuditRows: [
        { eventType: 'trial_started', payload: { device_id: 'device-Z' }, createdAt: new Date() },
      ],
    });
    const result = await startTrial(db, { device_id: 'device-Z' }, TEST_PRIVATE_KEY_PEM);
    expect(result.status).toBe('trial_resumed');
    expect((result as { trial_ends_at: string }).trial_ends_at).toBe(endsAt.toISOString());
    expect(db.subscription.create).not.toHaveBeenCalled();
  });

  it('a lapsed trial is signed as LOCKED', async () => {
    const { verifyLicenseJwt } = await import('../lib/jwt');
    const { TEST_PUBLIC_KEY_PEM } = await import('./testKeys');
    const db = makeDb({
      existingBinding: { accountId: 'acc_trial' },
      boundSubscription: {
        status: 'trialing',
        planId: 'desktop_pro_monthly',
        billingInterval: 'monthly',
        currentPeriodEnd: new Date(Date.now() - 1000),
      },
      priorDeviceTrialAuditRows: [
        { eventType: 'trial_started', payload: { device_id: 'device-Z' }, createdAt: new Date() },
      ],
    });
    const result = await startTrial(db, { device_id: 'device-Z' }, TEST_PRIVATE_KEY_PEM);
    expect(verifyLicenseJwt(result.jwt, TEST_PUBLIC_KEY_PEM).state).toBe('LOCKED');
  });
});
