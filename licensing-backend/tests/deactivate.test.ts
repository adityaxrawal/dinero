// Doc 30 TASK-LIC-004 acceptance criteria (corrected during TASK-BILL-002 --
// device_id-only input, matching the already-shipped desktop client, which
// reuses ValidateRequest{device_id} for deactivate too).
import { describe, it, expect, vi } from 'vitest';
import { deactivateLicense, type DeactivateDb } from '../api/license/deactivate';

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
    await expect(deactivateLicense(db, { device_id: 'device-UNKNOWN' })).rejects.toMatchObject({
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
    const result = await deactivateLicense(db, { device_id: 'device-A' });
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
    await deactivateLicense(db, { device_id: 'device-A' }, { send });
    expect(send).toHaveBeenCalledWith(expect.objectContaining({ to: 'user@example.com' }));
  });
});
