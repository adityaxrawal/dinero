// Review M16 follow-up: a reinstall that lost its token restores the licence
// with the account email and the purchase's payment id.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { restoreLicense, type RestoreDb } from '../api/license/restore';
import { verifyLicenseJwt } from '../lib/jwt';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';

function makeDb(opts: {
  account?: Record<string, unknown> | null;
  payment?: Record<string, unknown> | null;
  binding?: Record<string, unknown> | null;
  subscription?: Record<string, unknown> | null;
}): RestoreDb {
  return {
    account: { findUnique: vi.fn().mockResolvedValue(opts.account ?? null) } as unknown as RestoreDb['account'],
    paymentProviderRecord: {
      findFirst: vi.fn().mockResolvedValue(opts.payment ?? null),
    } as unknown as RestoreDb['paymentProviderRecord'],
    licenseToken: {
      findFirst: vi.fn().mockResolvedValue(opts.binding ?? null),
      upsert: vi.fn().mockResolvedValue({}),
      updateMany: vi.fn().mockResolvedValue({ count: 1 }),
    } as unknown as RestoreDb['licenseToken'],
    subscription: {
      findFirst: vi.fn().mockResolvedValue(opts.subscription ?? null),
    } as unknown as RestoreDb['subscription'],
    licensingAuditLog: auditLog(),
  };
}

// In-memory audit log that honours the eventType + createdAt window query.
function auditLog(): RestoreDb['licensingAuditLog'] {
  const rows: { eventType: string; payload: unknown; createdAt: Date }[] = [];
  return {
    create: vi.fn(async ({ data }: { data: { eventType: string; payload?: unknown } }) => {
      rows.push({ eventType: data.eventType, payload: data.payload ?? null, createdAt: new Date() });
      return {};
    }),
    findMany: vi.fn(async ({ where }: { where: { eventType: string; createdAt: { gte: Date } } }) =>
      rows.filter((r) => r.eventType === where.eventType && r.createdAt >= where.createdAt.gte)
    ),
  } as unknown as RestoreDb['licensingAuditLog'];
}

const input = { email: 'u@e.com', razorpay_payment_id: 'pay_1', device_id: 'device-A' };
const sub = { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active', currentPeriodEnd: null };

describe('restoreLicense', () => {
  it('re-binds this device and returns a signed token with the subscription state', async () => {
    const db = makeDb({
      account: { id: 'acc_1' },
      payment: { accountId: 'acc_1' },
      binding: { deviceFingerprint: 'device-A' },
      subscription: sub,
    });
    const r = await restoreLicense(db, input, TEST_PRIVATE_KEY_PEM);
    expect(r.state).toBe('ACTIVE');
    const claims = verifyLicenseJwt(r.jwt, TEST_PUBLIC_KEY_PEM);
    expect(claims.device_id).toBe('device-A');
    expect(claims.state).toBe('ACTIVE');
  });

  it('works when the old binding was cleared (no device bound)', async () => {
    const db = makeDb({
      account: { id: 'acc_1' },
      payment: { accountId: 'acc_1' },
      binding: { deviceFingerprint: null },
      subscription: sub,
    });
    await expect(restoreLicense(db, input, TEST_PRIVATE_KEY_PEM)).resolves.toMatchObject({ status: 'restored' });
  });

  it('refuses a payment id not recorded for that email, with a generic message', async () => {
    const db = makeDb({ account: { id: 'acc_1' }, payment: null, subscription: sub });
    await expect(restoreLicense(db, input, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({
      code: 'LICENSE_INVALID',
    });
    const noAccount = makeDb({});
    await expect(restoreLicense(noAccount, input, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({
      code: 'LICENSE_INVALID',
    });
  });

  describe('moving a licence bound to another device', () => {
    afterEach(() => vi.useRealTimers());
    const bound = () =>
      makeDb({
        account: { id: 'acc_1' },
        payment: { accountId: 'acc_1' },
        binding: { deviceFingerprint: 'device-B' },
        subscription: sub,
      });

    it('moves it here and revokes the old binding', async () => {
      const db = bound();
      const r = await restoreLicense(db, input, TEST_PRIVATE_KEY_PEM);
      expect(verifyLicenseJwt(r.jwt, TEST_PUBLIC_KEY_PEM).device_id).toBe('device-A');
      expect(db.licenseToken.updateMany).toHaveBeenCalledWith({
        where: { accountId: 'acc_1', NOT: { deviceFingerprint: 'device-A' } },
        data: { deviceFingerprint: null, deviceBoundAt: null, revokedAt: expect.any(Date) },
      });
      expect(db.licensingAuditLog.create).toHaveBeenCalledWith({
        data: expect.objectContaining({ accountId: 'acc_1', eventType: 'license_rebind' }),
      });
    });

    it('refuses a second move within 30 days, and allows one after', async () => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date('2026-01-01T00:00:00Z'));
      const db = bound();
      await restoreLicense(db, input, TEST_PRIVATE_KEY_PEM);

      vi.setSystemTime(new Date('2026-01-29T00:00:00Z'));
      await expect(
        restoreLicense(db, { ...input, device_id: 'device-C' }, TEST_PRIVATE_KEY_PEM)
      ).rejects.toMatchObject({ code: 'DEVICE_ALREADY_BOUND', message: expect.stringMatching(/support/i) });
      expect(db.licenseToken.updateMany).toHaveBeenCalledTimes(1);

      vi.setSystemTime(new Date('2026-02-01T00:00:01Z'));
      await expect(
        restoreLicense(db, { ...input, device_id: 'device-C' }, TEST_PRIVATE_KEY_PEM)
      ).resolves.toMatchObject({ status: 'restored' });
      expect(db.licenseToken.updateMany).toHaveBeenCalledTimes(2);
    });
  });
});
