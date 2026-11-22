// Doc 30 TASK-LIC-004 acceptance criteria (corrected during TASK-BILL-002 --
// device_id-only input, matching the already-shipped desktop client, which
// reuses ValidateRequest{device_id} for deactivate too).
import { describe, it, expect, vi } from 'vitest';
import { deactivateLicense, type DeactivateDb } from '../api/license/deactivate';
import { signLicenseJwt } from '../lib/jwt';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';

const proof = (deviceId: string) =>
  signLicenseJwt(
    { sub: 'acc_1', device_id: deviceId, plan: 'p', billing_interval: 'monthly', state: 'ACTIVE' },
    TEST_PRIVATE_KEY_PEM
  );

function makeDb(token: { id: string; accountId: string; account: { email: string } } | null) {
  const update = vi.fn().mockResolvedValue({});
  const db: DeactivateDb = {
    licenseToken: {
      findUnique: vi.fn().mockResolvedValue(token),
      update,
    } as unknown as DeactivateDb['licenseToken'],
    licensingAuditLog: {
      create: vi.fn().mockResolvedValue({}),
      findMany: vi.fn().mockResolvedValue([]),
    } as unknown as DeactivateDb['licensingAuditLog'],
  };
  return { db, update };
}

describe('test_deactivate_requires_bound_device', () => {
  it('rejects deactivation when no license_tokens row is bound to the requesting device', async () => {
    const { db } = makeDb(null);
    await expect(deactivateLicense(db, { device_id: 'device-UNKNOWN', jwt: proof('device-UNKNOWN') }, TEST_PUBLIC_KEY_PEM)).rejects.toMatchObject({
      code: 'LICENSE_INVALID',
    });
  });
});

describe('test_deactivate_frees_license_for_reactivation', () => {
  it('clears the device fingerprint so a future activation from a new device succeeds', async () => {
    const { db, update } = makeDb({
      id: 'lt_1',
      accountId: 'acc_1',
      account: { email: 'user@example.com' },
    });
    const result = await deactivateLicense(db, { device_id: 'device-A', jwt: proof('device-A') }, TEST_PUBLIC_KEY_PEM);
    expect(result.status).toBe('deactivated');
    expect(update).toHaveBeenCalledWith(
      expect.objectContaining({ data: expect.objectContaining({ deviceFingerprint: null }) })
    );
  });
});

describe('test_deactivate_sends_confirmation_email', () => {
  it('sends a confirmation email to the account address', async () => {
    const { db } = makeDb({
      id: 'lt_1',
      accountId: 'acc_1',
      account: { email: 'user@example.com' },
    });
    const send = vi.fn().mockResolvedValue(undefined);
    await deactivateLicense(db, { device_id: 'device-A', jwt: proof('device-A') }, TEST_PUBLIC_KEY_PEM, { send });
    expect(send).toHaveBeenCalledWith(expect.objectContaining({ to: 'user@example.com' }));
  });
});

// Review M16: a device id alone can no longer deactivate a licence.
describe('deactivate requires proof of the device token', () => {
  it("rejects another device's token and leaves the binding alone", async () => {
    const { db, update } = makeDb({ id: 'lt_1', accountId: 'acc_1', account: { email: 'u@e.com' } });
    await expect(
      deactivateLicense(db, { device_id: 'device-A', jwt: proof('device-B') }, TEST_PUBLIC_KEY_PEM)
    ).rejects.toMatchObject({ code: 'DEVICE_MISMATCH' });
    expect(update).not.toHaveBeenCalled();
  });
});
