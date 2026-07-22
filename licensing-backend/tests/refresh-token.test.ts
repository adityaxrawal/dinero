// Doc 30 TASK-LIC-008 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { refreshLicenseToken, type RefreshDb } from '../api/license/refresh-token';
import { signLicenseJwt, verifyLicenseJwt } from '../lib/jwt';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';

function makeDb(token: Record<string, unknown> | null, subscription: Record<string, unknown> | null): RefreshDb {
  return {
    licenseToken: { findFirst: vi.fn().mockResolvedValue(token) } as unknown as RefreshDb['licenseToken'],
    subscription: { findFirst: vi.fn().mockResolvedValue(subscription) } as unknown as RefreshDb['subscription'],
  };
}

function tokenExpiringInSeconds(seconds: number): string {
  return signLicenseJwt(
    { sub: 'user@example.com', device_id: 'device-A', plan: 'desktop_pro_monthly', billing_interval: 'monthly' },
    TEST_PRIVATE_KEY_PEM,
    seconds
  );
}

describe('test_refresh_issues_new_jwt_with_extended_expiry', () => {
  it('returns a fresh JWT with a later expiry than the original', async () => {
    const original = tokenExpiringInSeconds(60); // about to expire
    const db = makeDb(
      { id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A' },
      { planId: 'desktop_pro_monthly', billingInterval: 'monthly', status: 'active' }
    );
    const result = await refreshLicenseToken(db, { jwt: original, device_id: 'device-A' }, TEST_PUBLIC_KEY_PEM, TEST_PRIVATE_KEY_PEM);
    expect(result.status).toBe('refreshed');
    const originalClaims = verifyLicenseJwt(original, TEST_PUBLIC_KEY_PEM, { ignoreExpiration: true });
    const newClaims = verifyLicenseJwt(result.jwt, TEST_PUBLIC_KEY_PEM);
    expect(newClaims.exp).toBeGreaterThan(originalClaims.exp);
  });
});

describe('test_refresh_rejects_hardware_uuid_mismatch', () => {
  it('rejects when the requesting device does not match the JWT device_id claim', async () => {
    const original = tokenExpiringInSeconds(3600);
    const db = makeDb({ id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A' }, { planId: 'x', billingInterval: 'monthly', status: 'active' });
    await expect(refreshLicenseToken(db, { jwt: original, device_id: 'device-DIFFERENT' }, TEST_PUBLIC_KEY_PEM, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject(
      { code: 'DEVICE_MISMATCH' }
    );
  });
});

describe('test_refresh_rejects_beyond_max_staleness', () => {
  it('rejects a token that expired more than 48 hours ago', async () => {
    const staleToken = tokenExpiringInSeconds(-49 * 60 * 60); // expired 49h ago
    const db = makeDb({ id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A' }, { planId: 'x', billingInterval: 'monthly', status: 'active' });
    await expect(refreshLicenseToken(db, { jwt: staleToken, device_id: 'device-A' }, TEST_PUBLIC_KEY_PEM, TEST_PRIVATE_KEY_PEM)).rejects.toMatchObject({
      code: 'LICENSE_INVALID',
    });
  });

  it('allows a token expired less than 48 hours ago', async () => {
    const recentlyExpired = tokenExpiringInSeconds(-1 * 60 * 60); // expired 1h ago
    const db = makeDb({ id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A' }, { planId: 'x', billingInterval: 'monthly', status: 'active' });
    await expect(refreshLicenseToken(db, { jwt: recentlyExpired, device_id: 'device-A' }, TEST_PUBLIC_KEY_PEM, TEST_PRIVATE_KEY_PEM)).resolves.toMatchObject({
      status: 'refreshed',
    });
  });
});
