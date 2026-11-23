// Review M16 follow-up: a reinstall that lost its token restores the licence
// with the account email and the purchase's payment id.
import { describe, it, expect, vi } from 'vitest';
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
    } as unknown as RestoreDb['licenseToken'],
    subscription: {
      findFirst: vi.fn().mockResolvedValue(opts.subscription ?? null),
    } as unknown as RestoreDb['subscription'],
    licensingAuditLog: {
      create: vi.fn().mockResolvedValue({}),
      findMany: vi.fn().mockResolvedValue([]),
    } as unknown as RestoreDb['licensingAuditLog'],
  };
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

  it('refuses while the licence is bound to another device', async () => {
    const db = makeDb({
      account: { id: 'acc_1' },
      payment: { accountId: 'acc_1' },
      binding: { deviceFingerprint: 'device-B' },
      subscription: sub,
    });
    await expect(restoreLicense(db, input, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({
      code: 'DEVICE_ALREADY_BOUND',
    });
  });
});
