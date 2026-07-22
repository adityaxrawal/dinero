// Doc 30 TASK-LIC-004 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { deactivateLicense, type DeactivateDb } from '../api/license/deactivate';
import { hashLicenseKey } from '../lib/license_key';

const licenseKeyHash = hashLicenseKey('DINERO-TEST-KEY');

function makeDb(token: Record<string, unknown> | null) {
  const update = vi.fn().mockResolvedValue({});
  const db: DeactivateDb = {
    licenseToken: { findUnique: vi.fn().mockResolvedValue(token), update } as unknown as DeactivateDb['licenseToken'],
    licensingAuditLog: { create: vi.fn().mockResolvedValue({}), findMany: vi.fn().mockResolvedValue([]) } as unknown as DeactivateDb['licensingAuditLog'],
  };
  return { db, update };
}

describe('test_deactivate_requires_bound_device', () => {
  it('rejects deactivation requested from a device other than the bound one', async () => {
    const { db } = makeDb({ id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A', licenseKeyHash });
    await expect(
      deactivateLicense(db, { license_key: 'DINERO-TEST-KEY', device_id: 'device-STRANGER' }, 'user@example.com')
    ).rejects.toMatchObject({ code: 'DEVICE_MISMATCH' });
  });
});

describe('test_deactivate_frees_license_for_reactivation', () => {
  it('clears the device fingerprint so a future activation from a new device succeeds', async () => {
    const { db, update } = makeDb({ id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A', licenseKeyHash });
    const result = await deactivateLicense(db, { license_key: 'DINERO-TEST-KEY', device_id: 'device-A' }, 'user@example.com');
    expect(result.status).toBe('deactivated');
    expect(update).toHaveBeenCalledWith(
      expect.objectContaining({ data: expect.objectContaining({ deviceFingerprint: null }) })
    );
  });
});

describe('test_deactivate_sends_confirmation_email', () => {
  it('sends a confirmation email to the account address', async () => {
    const { db } = makeDb({ id: 'lt_1', accountId: 'acc_1', deviceFingerprint: 'device-A', licenseKeyHash });
    const send = vi.fn().mockResolvedValue(undefined);
    await deactivateLicense(db, { license_key: 'DINERO-TEST-KEY', device_id: 'device-A' }, 'user@example.com', { send });
    expect(send).toHaveBeenCalledWith(expect.objectContaining({ to: 'user@example.com' }));
  });
});
