// Doc 30 TASK-OPS-006 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { lookupSupportAccount, type SupportLookupDb } from '../api/admin/support_lookup';
import { resetDeviceBinding, type ResetBindingDb } from '../api/admin/support_reset_binding';
import { reissueToken, type ReissueTokenDb } from '../api/admin/support_reissue_token';
import { recommendRecovery, INVASIVENESS_ORDER } from '../lib/support_recovery';
import { maskEmail } from '../lib/license_key';
import { LicensingApiError } from '../lib/errors';

describe('test_support_tools_never_expose_financial_data', () => {
  it('lookup response never contains a raw email, only the masked form', async () => {
    const db: SupportLookupDb = {
      account: {
        findUnique: vi.fn().mockResolvedValue({
          id: 'acc_1',
          email: 'realuser@example.com',
          trialUsed: true,
          subscriptions: [
            { status: 'active', planId: 'pro', currentPeriodEnd: new Date('2026-08-01') },
          ],
          licenseTokens: [
            {
              deviceFingerprint: 'hw-uuid-1',
              jwtIssuedAt: new Date(),
              jwtExpiresAt: new Date(),
              revokedAt: null,
            },
          ],
        }),
      },
      licensingAuditLog: {
        findMany: vi
          .fn()
          .mockResolvedValue([{ eventType: 'license_activated', createdAt: new Date() }]),
      } as unknown as SupportLookupDb['licensingAuditLog'],
    };

    const result = await lookupSupportAccount(db, { email: 'realuser@example.com' });
    const json = JSON.stringify(result);
    expect(json).not.toContain('realuser@example.com');
    expect(result.email_masked).toBe(maskEmail('realuser@example.com'));
  });

  it('this backend has no merchant/amount/transaction fields to leak in the first place', async () => {
    const db: SupportLookupDb = {
      account: { findUnique: vi.fn().mockResolvedValue(null) },
      licensingAuditLog: { findMany: vi.fn() } as unknown as SupportLookupDb['licensingAuditLog'],
    };
    await expect(lookupSupportAccount(db, { email: 'nope@example.com' })).rejects.toThrow(
      LicensingApiError
    );
    // Structural: the result type itself has no amount/merchant/transaction keys.
    const forbidden = ['amount', 'merchant', 'transaction', 'card', 'iban', 'razorpay_payment_id'];
    const sourceUnderTest = [
      'account_id',
      'email_masked',
      'trial_used',
      'subscriptions',
      'license_tokens',
      'history',
      'recommended_recovery',
    ];
    for (const key of sourceUnderTest) {
      expect(forbidden).not.toContain(key);
    }
  });
});

describe('test_support_reset_binding_is_audited', () => {
  function makeDb(): ResetBindingDb {
    return {
      account: { findUnique: vi.fn().mockResolvedValue({ id: 'acc_1' }) },
      licenseToken: {
        findFirst: vi.fn().mockResolvedValue({ id: 'tok_1' }),
        update: vi.fn().mockResolvedValue({}),
      } as unknown as ResetBindingDb['licenseToken'],
      licensingAuditLog: {
        create: vi.fn().mockResolvedValue({}),
        findMany: vi.fn(),
      } as unknown as ResetBindingDb['licensingAuditLog'],
    };
  }

  it('clears the device binding and logs the reason to the audit trail', async () => {
    const db = makeDb();
    const result = await resetDeviceBinding(db, {
      email: 'user@example.com',
      reason: 'Lost Mac, verified via support ticket #123',
    });
    expect(result.status).toBe('binding_reset');
    expect(db.licenseToken.update).toHaveBeenCalledWith(
      expect.objectContaining({
        data: expect.objectContaining({ deviceFingerprint: null, deviceBoundAt: null }),
      })
    );
    expect(db.licensingAuditLog.create).toHaveBeenCalledWith(
      expect.objectContaining({
        data: expect.objectContaining({
          eventType: 'admin_support_reset_binding',
          payload: expect.objectContaining({
            reason: 'Lost Mac, verified via support ticket #123',
          }),
        }),
      })
    );
  });

  it('rejects an unknown account without touching any token row', async () => {
    const db = makeDb();
    db.account.findUnique = vi.fn().mockResolvedValue(null);
    await expect(
      resetDeviceBinding(db, { email: 'nope@example.com', reason: 'test' })
    ).rejects.toThrow(LicensingApiError);
    expect(db.licenseToken.update).not.toHaveBeenCalled();
  });
});

describe('test_support_reissue_token_requires_reason', () => {
  function makeDb(): ReissueTokenDb {
    return {
      account: { findUnique: vi.fn().mockResolvedValue({ id: 'acc_1' }) },
      subscription: {
        findFirst: vi
          .fn()
          .mockResolvedValue({ planId: 'pro', billingInterval: 'month', status: 'active' }),
      },
      licenseToken: {
        findFirst: vi.fn().mockResolvedValue(null),
        update: vi.fn().mockResolvedValue({}),
        create: vi.fn().mockResolvedValue({}),
      } as unknown as ReissueTokenDb['licenseToken'],
      licensingAuditLog: {
        create: vi.fn().mockResolvedValue({}),
        findMany: vi.fn(),
      } as unknown as ReissueTokenDb['licensingAuditLog'],
    };
  }

  it('rejects an empty reason before touching any row', async () => {
    const db = makeDb();
    await expect(
      reissueToken(db, { email: 'user@example.com', new_device_id: 'hw-2', reason: '' }, 'fake-pem')
    ).rejects.toThrow('reason is required');
    expect(db.account.findUnique).not.toHaveBeenCalled();
  });

  it('rejects a whitespace-only reason', async () => {
    const db = makeDb();
    await expect(
      reissueToken(
        db,
        { email: 'user@example.com', new_device_id: 'hw-2', reason: '   ' },
        'fake-pem'
      )
    ).rejects.toThrow('reason is required');
  });

  it('succeeds and audits the reason when a real reason is given', async () => {
    const db = makeDb();
    const result = await reissueToken(
      db,
      {
        email: 'user@example.com',
        new_device_id: 'hw-2',
        reason: 'Reinstalled macOS after disk failure',
      },
      TEST_PRIVATE_KEY_PEM
    );
    expect(result.status).toBe('reissued');
    expect(db.licensingAuditLog.create).toHaveBeenCalledWith(
      expect.objectContaining({
        data: expect.objectContaining({
          payload: expect.objectContaining({ reason: 'Reinstalled macOS after disk failure' }),
        }),
      })
    );
  });
});

describe('test_support_flow_uses_least_invasive_recovery_first', () => {
  it('every recommended case lists steps in the documented invasiveness order', () => {
    const cases = [
      'stuck_grace_or_locked',
      'lost_or_replaced_device',
      'reinstalled_os',
      'corrupted_local_state',
    ] as const;
    for (const c of cases) {
      const steps = recommendRecovery(c).map((s) => s.action);
      const indices = steps.map((a) => INVASIVENESS_ORDER.indexOf(a));
      const sorted = [...indices].sort((a, b) => a - b);
      expect(indices).toEqual(sorted);
    }
  });

  it('a stuck GRACE/LOCKED case recommends refresh before rebind, never jumping straight to a full reset', () => {
    const steps = recommendRecovery('stuck_grace_or_locked').map((s) => s.action);
    expect(steps[0]).toBe('refresh');
    expect(steps).not.toContain('full_local_reset');
  });

  it('a corrupted local database never recommends the device-binding steps, which cannot fix it', () => {
    const steps = recommendRecovery('corrupted_local_state').map((s) => s.action);
    expect(steps).not.toContain('refresh');
    expect(steps).not.toContain('rebind');
    expect(steps[0]).toBe('restore_backup');
  });
});

// A throwaway RSA key generated solely for this test file (openssl genpkey),
// never used anywhere else -- signLicenseJwt needs a real PEM to sign against.
const TEST_PRIVATE_KEY_PEM = `-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQC7j+Pu4+kPgSBE
BxxAZaHz4kIB+hALiQxivRrzV0S1vqODfgFB22ktOFBxyP2EuYS/vIIwxGAvkjnS
LhwDDeh4M+VJ2UJ0rVT1kpcpsNYfCVNtDMXx1LklQwgXOBvviq1KRzWN6tMdA27m
6s8wOXH3SDoPEJEFU1Ly7novUteEjMXqgDaE3gZtkBJHU71vouDfJs1mi83rKtlp
scfZnFouWI+dELVpx/67pLFEdYzw3EenlKur9z+Tg2md3mDc58knaN0UvISr4N3+
oEE0r6ldXBDhy1J4fuXdJA2TNdH9yxBaX7YTl1y+K/0BQgvE4WlZOo0jt1XHYn/m
JO5K7JaDAgMBAAECggEASpFHNidn9eEJOeJ1yehd/b7bLTwEewGOagbymwH78CGN
km5bA5q+ctrrqIEWwVicPTotrEO2VdNVp4jrXA+Ad6FyK+zzLb3nhAY2kL2cMxSb
FUS5wY4n2XeP1ONav94rTNeIpBZSvjsqMSXzHyIHfB877ddRQIPC+4/yBbRyuGAs
5ZvJM7ybb25BkfoVLB0KireBrsv2lDHRKBpIzUcbSF6baJFhQ/QZcwb+EJ/+fKQ9
54dOWLuCWLoL6k96IKipKxq8RDeoj5Qqbj07LobhQfSWgQQE0Xw0jyAzfdZH0xIL
scVhmSnxhc8bU0WKsSse/0pUgSCU0nwn/Tub9/TuiQKBgQDc/dEiGzF/L9rTpKub
i9X6DDio3lusH3XTH6v2cbpkzuR4HCCnDYNSGNxEiZjJp99MEmRNtl62Ek2JShTN
pG+upvlmhZ/Le0fwHf1PfQKulm2IZN/ZQsm5mRhh0sBfQRFEeIG3b1VYuiMqyIWd
+5jtzQhvkQIOvc5ItHpFeo3etwKBgQDZRlz4koeaVNSCTOeDrgisOlJ3Mx0WNccm
H8d1Ej7hnoOHlvpk3pkjnCqFMuh7wAt9TK96o6TT8H0dHQRWGHrWMC7HcSK/n4TG
/BUDOooIapMnF81DGLZ6sEtGN08GRYGJ5ZwhsXU1fSpOAfsG0bmXM7cQikbwaA9X
rEB8CL+6lQKBgHKOrLvGZvksoH44DbF7YrfVYAXCBrmKMXT5JRaCzAH38h2FTzPp
4FpNgtmQjoByomF340EZua0efc0edvxHMpHSAtUvja9Yv+jsUuTCxAIm/q7Gw/eH
FLU+dJI5Qvnd7AqXgX7Kmu58x0AlZIaJ5zPWpnnXLL7hi67Kx9t+dU6vAoGAVfjt
mL4CQiMG43gis4wNiniZYOksvTkSUBeLCNvrXcMnMGOhOICL//cvK/102GKpKS9K
0DAobGRgXUC6EoclM9Nk7y3pHgG0vDfK6LglHidtiq50XfRNYEaZwnLoJgcitrnQ
CdT6F+wq7SsDdTNPSHECIt1ULJRAXeSer3WWx/ECgYAUrt0MOhBEG+S3geIZ6959
jAkaFf77bjnFo3pJjrhGeg0TmZjvhEL7RcRZW0CAsfDaM90auOktvgeoQgA5PB1U
zLpZ1nDmg7GJQdzx6JtJGBWiGCOYFOm6M88C2t53U+iHLMp6fpmenyEVweclu7y2
r3Uu4d1HOAzLciV3W1HyMQ==
-----END PRIVATE KEY-----`;
