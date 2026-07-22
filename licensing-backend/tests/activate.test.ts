// Doc 30 TASK-LIC-002 acceptance criteria. Uses a hand-rolled mock DB (not a
// live Postgres/Neon connection) -- activateLicense only depends on the
// narrow ActivateDb interface, so these exercise the real business logic
// without needing a database at all.
import { describe, it, expect, vi } from 'vitest';
import { activateLicense, type ActivateDb } from '../api/license/activate';
import { hashLicenseKey } from '../lib/license_key';
import { LicensingApiError } from '../lib/errors';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';
import { verifyLicenseJwt } from '../lib/jwt';

function makeDb(overrides: {
  token: Record<string, unknown> | null;
  subscription: Record<string, unknown> | null;
  auditRows?: { eventType: string; payload: unknown; createdAt: Date }[];
}): ActivateDb {
  const auditRows = overrides.auditRows ?? [];
  return {
    licenseToken: {
      findUnique: vi.fn().mockResolvedValue(overrides.token),
      update: vi.fn().mockResolvedValue({}),
    } as unknown as ActivateDb['licenseToken'],
    subscription: {
      findFirst: vi.fn().mockResolvedValue(overrides.subscription),
    } as unknown as ActivateDb['subscription'],
    licensingAuditLog: {
      create: vi.fn().mockImplementation(({ data }) => {
        auditRows.push({ eventType: data.eventType, payload: data.payload, createdAt: new Date() });
        return Promise.resolve({});
      }),
      findMany: vi.fn().mockImplementation(() => Promise.resolve(auditRows)),
    } as unknown as ActivateDb['licensingAuditLog'],
  };
}

const baseInput = { license_key: 'DINERO-TEST-KEY', device_id: 'device-A', email: 'user@example.com' };

describe('test_activation_binds_new_device', () => {
  it('binds the device and issues a valid JWT on first activation', async () => {
    const db = makeDb({
      token: { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: null, licenseKeyHash: hashLicenseKey(baseInput.license_key) },
      subscription: { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' },
    });
    const result = await activateLicense(db, baseInput, TEST_PRIVATE_KEY_PEM);
    expect(result.status).toBe('activated');
    const verified = verifyLicenseJwt(result.jwt, TEST_PUBLIC_KEY_PEM);
    expect(verified.device_id).toBe('device-A');
    expect(db.licenseToken.update).toHaveBeenCalledWith(
      expect.objectContaining({ data: expect.objectContaining({ deviceFingerprint: 'device-A' }) })
    );
  });
});

describe('test_activation_rejects_second_device', () => {
  it('rejects activation from a different device once already bound', async () => {
    const db = makeDb({
      token: { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-ORIGINAL', licenseKeyHash: hashLicenseKey(baseInput.license_key) },
      subscription: { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' },
    });
    await expect(activateLicense(db, { ...baseInput, device_id: 'device-B' }, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({
      code: 'DEVICE_ALREADY_BOUND',
    });
  });
});

describe('test_expired_key_rejected', () => {
  it('rejects a license whose subscription is expired', async () => {
    const db = makeDb({
      token: { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: null, licenseKeyHash: hashLicenseKey(baseInput.license_key) },
      subscription: { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'expired' },
    });
    await expect(activateLicense(db, baseInput, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({ code: 'LICENSE_INVALID' });
  });

  it('rejects an unknown license key', async () => {
    const db = makeDb({ token: null, subscription: null });
    await expect(activateLicense(db, baseInput, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({ code: 'LICENSE_INVALID' });
  });
});

describe('test_rate_limit_enforced', () => {
  it('rejects the 7th activation attempt for the same key within an hour', async () => {
    const licenseKeyHash = hashLicenseKey(baseInput.license_key);
    const auditRows = Array.from({ length: 5 }, () => ({
      eventType: 'activation_attempt',
      payload: { license_key_hash: licenseKeyHash, device_id: 'device-A' },
      createdAt: new Date(),
    }));
    const db = makeDb({
      token: { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: null, licenseKeyHash },
      subscription: { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' },
      auditRows,
    });
    await expect(activateLicense(db, baseInput, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({ code: 'RATE_LIMITED' });
  });

  it('allows activation when attempt count is within the limit', async () => {
    const licenseKeyHash = hashLicenseKey(baseInput.license_key);
    const auditRows = Array.from({ length: 2 }, () => ({
      eventType: 'activation_attempt',
      payload: { license_key_hash: licenseKeyHash, device_id: 'device-A' },
      createdAt: new Date(),
    }));
    const db = makeDb({
      token: { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: null, licenseKeyHash },
      subscription: { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' },
      auditRows,
    });
    await expect(activateLicense(db, baseInput, TEST_PRIVATE_KEY_PEM)).resolves.toMatchObject({ status: 'activated' });
  });
});

it('LicensingApiError is the real thrown type', () => {
  expect(new LicensingApiError('LICENSE_INVALID', 'x')).toBeInstanceOf(LicensingApiError);
});
