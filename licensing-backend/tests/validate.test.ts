// Doc 30 TASK-LIC-003 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { validateLicense, type ValidateDb } from '../api/license/validate';
import { hashLicenseKey } from '../lib/license_key';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';
import { verifyLicenseJwt } from '../lib/jwt';

function makeDb(token: Record<string, unknown> | null, subscription: Record<string, unknown> | null): ValidateDb {
  return {
    licenseToken: { findUnique: vi.fn().mockResolvedValue(token) } as unknown as ValidateDb['licenseToken'],
    subscription: { findFirst: vi.fn().mockResolvedValue(subscription) } as unknown as ValidateDb['subscription'],
    licensingAuditLog: { create: vi.fn().mockResolvedValue({}), findMany: vi.fn().mockResolvedValue([]) } as unknown as ValidateDb['licensingAuditLog'],
  };
}

const licenseKeyHash = hashLicenseKey('DINERO-TEST-KEY');

describe('test_validate_returns_fresh_jwt_with_server_timestamp', () => {
  it('returns a newly signed JWT and an ISO server_time', async () => {
    const db = makeDb(
      { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A', licenseKeyHash },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' }
    );
    const before = Date.now();
    const result = await validateLicense(db, { license_key: 'DINERO-TEST-KEY', device_id: 'device-A' }, TEST_PRIVATE_KEY_PEM, 'user@example.com');
    expect(new Date(result.server_time).getTime()).toBeGreaterThanOrEqual(before);
    const verified = verifyLicenseJwt(result.jwt, TEST_PUBLIC_KEY_PEM);
    expect(verified.device_id).toBe('device-A');
  });
});

describe('test_device_mismatch_rejected', () => {
  it('rejects validation from a device that does not match the bound fingerprint', async () => {
    const db = makeDb(
      { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-ORIGINAL', licenseKeyHash },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' }
    );
    await expect(
      validateLicense(db, { license_key: 'DINERO-TEST-KEY', device_id: 'device-DIFFERENT' }, TEST_PRIVATE_KEY_PEM, 'user@example.com')
    ).rejects.toMatchObject({ code: 'DEVICE_MISMATCH' });
  });
});

describe('test_state_transitions_computed_correctly', () => {
  it.each([
    ['trialing', 'TRIAL'],
    ['active', 'ACTIVE'],
    ['past_due', 'PAST_DUE'],
    ['canceled', 'LOCKED'],
    ['expired', 'LOCKED'],
  ])('subscription.status=%s -> state=%s', async (dbStatus, expectedState) => {
    const db = makeDb(
      { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A', licenseKeyHash },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: dbStatus }
    );
    const result = await validateLicense(db, { license_key: 'DINERO-TEST-KEY', device_id: 'device-A' }, TEST_PRIVATE_KEY_PEM, 'user@example.com');
    expect(result.state).toBe(expectedState);
  });
});
