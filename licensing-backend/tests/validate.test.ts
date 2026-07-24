// Doc 30 TASK-LIC-003 acceptance criteria (corrected during TASK-BILL-002 --
// device_id-only input, matching the already-shipped desktop ValidateRequest).
import { describe, it, expect, vi } from 'vitest';
import { validateLicense, type ValidateDb } from '../api/license/validate';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';
import { verifyLicenseJwt } from '../lib/jwt';

function makeDb(
  token: { accountId: string; account: { email: string } } | null,
  subscription: Record<string, unknown> | null
): ValidateDb {
  return {
    licenseToken: {
      findUnique: vi.fn().mockResolvedValue(token),
    } as unknown as ValidateDb['licenseToken'],
    subscription: {
      findFirst: vi.fn().mockResolvedValue(subscription),
    } as unknown as ValidateDb['subscription'],
    licensingAuditLog: {
      create: vi.fn().mockResolvedValue({}),
      findMany: vi.fn().mockResolvedValue([]),
    } as unknown as ValidateDb['licensingAuditLog'],
  };
}

describe('test_validate_returns_fresh_jwt_with_server_timestamp', () => {
  it('returns a newly signed JWT and an ISO server_time', async () => {
    const db = makeDb(
      { accountId: 'acc_1', account: { email: 'user@example.com' } },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' }
    );
    const before = Date.now();
    const result = await validateLicense(db, { device_id: 'device-A' }, TEST_PRIVATE_KEY_PEM);
    expect(new Date(result.server_time).getTime()).toBeGreaterThanOrEqual(before);
    const verified = verifyLicenseJwt(result.jwt, TEST_PUBLIC_KEY_PEM);
    expect(verified.device_id).toBe('device-A');
    expect(verified.sub).toBe('user@example.com');
  });
});

describe('test_device_mismatch_rejected', () => {
  it('rejects when no license_tokens row is bound to the requesting device', async () => {
    const db = makeDb(null, null);
    await expect(
      validateLicense(db, { device_id: 'device-UNKNOWN' }, TEST_PRIVATE_KEY_PEM)
    ).rejects.toMatchObject({ code: 'LICENSE_INVALID' });
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
      { accountId: 'acc_1', account: { email: 'user@example.com' } },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: dbStatus }
    );
    const result = await validateLicense(db, { device_id: 'device-A' }, TEST_PRIVATE_KEY_PEM);
    expect(result.state).toBe(expectedState);
  });
});
