// Doc 30 TASK-LIC-003 acceptance criteria (corrected during TASK-BILL-002 --
// device_id-only input, matching the already-shipped desktop ValidateRequest).
import { describe, it, expect, vi } from 'vitest';
import { validateLicense, type ValidateDb } from '../api/license/validate';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';
import { verifyLicenseJwt, signLicenseJwt } from '../lib/jwt';

/** A token the device would hold, as proof for validate (review M16). */
const proof = (deviceId: string, expirySeconds = 3600) =>
  signLicenseJwt(
    { sub: 'acc_1', device_id: deviceId, plan: 'p', billing_interval: 'monthly', state: 'ACTIVE' },
    TEST_PRIVATE_KEY_PEM,
    expirySeconds
  );

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
    const result = await validateLicense(db, { device_id: 'device-A', jwt: proof('device-A') }, TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM);
    expect(new Date(result.server_time).getTime()).toBeGreaterThanOrEqual(before);
    const verified = verifyLicenseJwt(result.jwt, TEST_PUBLIC_KEY_PEM);
    expect(verified.device_id).toBe('device-A');
    expect(verified.sub).toBe('acc_1');
    expect(result.jwt).not.toContain('user@example.com');
  });
});

describe('test_device_mismatch_rejected', () => {
  it('rejects when no license_tokens row is bound to the requesting device', async () => {
    const db = makeDb(null, null);
    await expect(
      validateLicense(db, { device_id: 'device-UNKNOWN', jwt: proof('device-UNKNOWN') }, TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM)
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
    const result = await validateLicense(db, { device_id: 'device-A', jwt: proof('device-A') }, TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM);
    expect(result.state).toBe(expectedState);
  });
});

// Review M16: a device id alone no longer yields a token.
describe('validate requires proof of the device token', () => {
  const db = () =>
    makeDb(
      { accountId: 'acc_1', account: { email: 'user@example.com' } },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' }
    );
  it('rejects a missing token', async () => {
    await expect(
      validateLicense(db(), { device_id: 'device-A', jwt: '' }, TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM)
    ).rejects.toMatchObject({ code: 'LICENSE_INVALID' });
  });
  it("rejects another device's token", async () => {
    await expect(
      validateLicense(
        db(),
        { device_id: 'device-A', jwt: proof('device-B') },
        TEST_PRIVATE_KEY_PEM,
        TEST_PUBLIC_KEY_PEM
      )
    ).rejects.toMatchObject({ code: 'DEVICE_MISMATCH' });
  });
  it('accepts an expired token from the same device (offline recovery)', async () => {
    const result = await validateLicense(
      db(),
      { device_id: 'device-A', jwt: proof('device-A', -60) },
      TEST_PRIVATE_KEY_PEM,
      TEST_PUBLIC_KEY_PEM
    );
    expect(result.state).toBe('ACTIVE');
  });
});

// Rollout: older desktop builds send no token. Accepted only while
// LEGACY_NO_PROOF_UNTIL is in the future; a token, when sent, is always checked.
describe('legacy no-proof window', () => {
  const db = () =>
    makeDb(
      { accountId: 'acc_1', account: { email: 'user@example.com' } },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' }
    );
  it('accepts a missing token only while the window is open', async () => {
    vi.stubEnv('LEGACY_NO_PROOF_UNTIL', new Date(Date.now() + 86_400_000).toISOString());
    await expect(
      validateLicense(db(), { device_id: 'device-A', jwt: '' }, TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM)
    ).resolves.toMatchObject({ state: 'ACTIVE' });
    vi.stubEnv('LEGACY_NO_PROOF_UNTIL', new Date(Date.now() - 1000).toISOString());
    await expect(
      validateLicense(db(), { device_id: 'device-A', jwt: '' }, TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM)
    ).rejects.toMatchObject({ code: 'LICENSE_INVALID' });
    vi.unstubAllEnvs();
  });
  it("still rejects another device's token during the window", async () => {
    vi.stubEnv('LEGACY_NO_PROOF_UNTIL', new Date(Date.now() + 86_400_000).toISOString());
    await expect(
      validateLicense(db(), { device_id: 'device-A', jwt: proof('device-B') }, TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM)
    ).rejects.toMatchObject({ code: 'DEVICE_MISMATCH' });
    vi.unstubAllEnvs();
  });
});
