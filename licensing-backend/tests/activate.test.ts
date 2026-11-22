// Doc 30 TASK-LIC-002 acceptance criteria (corrected during TASK-BILL-002 --
// no license_key; activation is a Razorpay payment confirmation, matching
// the already-shipped desktop client exactly).
import { describe, it, expect, vi } from 'vitest';
import { activateLicense, type ActivateDb } from '../api/license/activate';
import { LicensingApiError } from '../lib/errors';
import { verifyLicenseJwt } from '../lib/jwt';
import { TEST_PRIVATE_KEY_PEM, TEST_PUBLIC_KEY_PEM } from './testKeys';
import { verifyPaymentSignature } from '../lib/razorpay';
import { createHmac } from 'node:crypto';

const RAZORPAY_KEY_SECRET = 'test-razorpay-secret';
const ORDER_ID = 'order_abc123';
const PAYMENT_ID = 'pay_xyz789';
const VALID_SIGNATURE = (() => {
  return createHmac('sha256', RAZORPAY_KEY_SECRET)
    .update(`${ORDER_ID}|${PAYMENT_ID}`)
    .digest('hex');
})();

function fakeRazorpayPayments(orderId = ORDER_ID) {
  return { fetch: vi.fn().mockResolvedValue({ orderId, amount: 29900, status: 'captured' }) };
}

function makeDb(overrides: {
  account: Record<string, unknown> | null;
  currentBinding: Record<string, unknown> | null;
  existingSubscription?: Record<string, unknown> | null;
  auditRows?: { eventType: string; payload: unknown; createdAt: Date }[];
  paymentRecord?: { accountId: string } | null;
}): ActivateDb {
  const auditRows = overrides.auditRows ?? [];
  return {
    account: {
      findUnique: vi.fn().mockResolvedValue(overrides.account),
      create: vi
        .fn()
        .mockResolvedValue({ id: 'acc_new', email: 'user@example.com', trialUsed: false }),
    } as unknown as ActivateDb['account'],
    plan: {
      findUnique: vi.fn().mockResolvedValue({ id: 'desktop_pro_monthly', amountMinor: 29900 }),
    } as unknown as ActivateDb['plan'],
    paymentProviderRecord: {
      findFirst: vi.fn().mockResolvedValue(overrides.paymentRecord ?? null),
      create: vi.fn().mockResolvedValue({}),
    } as unknown as ActivateDb['paymentProviderRecord'],
    licenseToken: {
      findFirst: vi.fn().mockResolvedValue(overrides.currentBinding),
      upsert: vi.fn().mockResolvedValue({}),
    } as unknown as ActivateDb['licenseToken'],
    subscription: {
      findFirst: vi.fn().mockResolvedValue(
        overrides.existingSubscription ?? {
          planId: 'desktop_pro_monthly',
          billingInterval: 'monthly',
          status: 'active',
        }
      ),
      create: vi.fn().mockResolvedValue({}),
    } as unknown as ActivateDb['subscription'],
    licensingAuditLog: {
      create: vi.fn().mockImplementation(({ data }) => {
        auditRows.push({ eventType: data.eventType, payload: data.payload, createdAt: new Date() });
        return Promise.resolve({});
      }),
      findMany: vi
        .fn()
        .mockImplementation(({ where }: { where: { eventType: string } }) =>
          Promise.resolve(auditRows.filter((r) => r.eventType === where.eventType))
        ),
    } as unknown as ActivateDb['licensingAuditLog'],
  };
}

const baseInput = {
  email: 'user@example.com',
  razorpay_payment_id: PAYMENT_ID,
  razorpay_signature: VALID_SIGNATURE,
  device_id: 'device-A',
  billing_interval: 'monthly' as const,
};

describe('test_activation_binds_new_device', () => {
  it('binds the device and issues a valid JWT on first activation', async () => {
    const db = makeDb({
      account: { id: 'acc_1', email: 'user@example.com', trialUsed: false },
      currentBinding: null,
    });
    const result = await activateLicense(
      db,
      baseInput,
      TEST_PRIVATE_KEY_PEM,
      RAZORPAY_KEY_SECRET,
      fakeRazorpayPayments()
    );
    expect(result.status).toBe('activated');
    const verified = verifyLicenseJwt(result.jwt, TEST_PUBLIC_KEY_PEM);
    expect(verified.device_id).toBe('device-A');
    expect(db.licenseToken.upsert).toHaveBeenCalledWith(
      expect.objectContaining({ where: { deviceFingerprint: 'device-A' } })
    );
  });
});

describe('test_activation_rejects_second_device', () => {
  it('rejects activation from a different device once already bound', async () => {
    const db = makeDb({
      account: { id: 'acc_1', email: 'user@example.com', trialUsed: false },
      currentBinding: { deviceFingerprint: 'device-ORIGINAL' },
    });
    await expect(
      activateLicense(
        db,
        { ...baseInput, device_id: 'device-B' },
        TEST_PRIVATE_KEY_PEM,
        RAZORPAY_KEY_SECRET,
        fakeRazorpayPayments()
      )
    ).rejects.toMatchObject({ code: 'DEVICE_ALREADY_BOUND' });
  });

  it('allows re-activation from the SAME already-bound device (idempotent)', async () => {
    const db = makeDb({
      account: { id: 'acc_1', email: 'user@example.com', trialUsed: false },
      currentBinding: { deviceFingerprint: 'device-A' },
    });
    await expect(
      activateLicense(
        db,
        baseInput,
        TEST_PRIVATE_KEY_PEM,
        RAZORPAY_KEY_SECRET,
        fakeRazorpayPayments()
      )
    ).resolves.toMatchObject({ status: 'activated' });
  });
});

describe('test_expired_key_rejected', () => {
  it('rejects when Razorpay payment signature verification fails', async () => {
    const db = makeDb({
      account: { id: 'acc_1', email: 'user@example.com', trialUsed: false },
      currentBinding: null,
    });
    await expect(
      activateLicense(
        db,
        { ...baseInput, razorpay_signature: 'not-a-real-signature-0000000000' },
        TEST_PRIVATE_KEY_PEM,
        RAZORPAY_KEY_SECRET,
        fakeRazorpayPayments()
      )
    ).rejects.toMatchObject({ code: 'PAYMENT_VERIFICATION_FAILED' });
  });

  it('rejects an unknown billing_interval', async () => {
    const db = makeDb({
      account: { id: 'acc_1', email: 'user@example.com', trialUsed: false },
      currentBinding: null,
    });
    await expect(
      activateLicense(
        db,
        { ...baseInput, billing_interval: 'weekly' as never },
        TEST_PRIVATE_KEY_PEM,
        RAZORPAY_KEY_SECRET,
        fakeRazorpayPayments()
      )
    ).rejects.toMatchObject({ code: 'VALIDATION_ERROR' });
  });
});

describe('test_rate_limit_enforced', () => {
  it('rejects the 7th activation attempt for the same email within an hour', async () => {
    const auditRows = Array.from({ length: 5 }, () => ({
      eventType: 'activation_attempt',
      payload: { email: baseInput.email, device_id: 'device-A' },
      createdAt: new Date(),
    }));
    const db = makeDb({
      account: { id: 'acc_1', email: 'user@example.com', trialUsed: false },
      currentBinding: null,
      auditRows,
    });
    await expect(
      activateLicense(
        db,
        baseInput,
        TEST_PRIVATE_KEY_PEM,
        RAZORPAY_KEY_SECRET,
        fakeRazorpayPayments()
      )
    ).rejects.toMatchObject({ code: 'RATE_LIMITED' });
  });

  it('allows activation when attempt count is within the limit', async () => {
    const auditRows = Array.from({ length: 2 }, () => ({
      eventType: 'activation_attempt',
      payload: { email: baseInput.email, device_id: 'device-A' },
      createdAt: new Date(),
    }));
    const db = makeDb({
      account: { id: 'acc_1', email: 'user@example.com', trialUsed: false },
      currentBinding: null,
      auditRows,
    });
    await expect(
      activateLicense(
        db,
        baseInput,
        TEST_PRIVATE_KEY_PEM,
        RAZORPAY_KEY_SECRET,
        fakeRazorpayPayments()
      )
    ).resolves.toMatchObject({ status: 'activated' });
  });
});

describe('payment signature verification (real HMAC, no mock)', () => {
  it('verifyPaymentSignature validates the exact fixture used above', () => {
    expect(verifyPaymentSignature(ORDER_ID, PAYMENT_ID, VALID_SIGNATURE, RAZORPAY_KEY_SECRET)).toBe(
      true
    );
  });
});

it('LicensingApiError is the real thrown type', () => {
  expect(new LicensingApiError('LICENSE_INVALID', 'x')).toBeInstanceOf(LicensingApiError);
});

// Review H1: a payment must be complete, for the plan's price, and used once.
describe('payment checks on activation', () => {
  const account = { id: 'acc_1', email: 'user@example.com', trialUsed: false };
  const run = (db: ActivateDb, payment: Record<string, unknown>) =>
    activateLicense(db, baseInput, TEST_PRIVATE_KEY_PEM, RAZORPAY_KEY_SECRET, {
      fetch: vi.fn().mockResolvedValue({ orderId: ORDER_ID, ...payment }),
    });

  it('rejects a payment that is not captured or authorized', async () => {
    const db = makeDb({ account, currentBinding: null });
    await expect(run(db, { amount: 29900, status: 'failed' })).rejects.toThrow(/not complete/);
  });

  it('rejects a payment whose amount does not match the plan', async () => {
    const db = makeDb({ account, currentBinding: null });
    await expect(run(db, { amount: 100, status: 'captured' })).rejects.toThrow(/amount/);
  });

  it('rejects a payment already used by another account', async () => {
    const db = makeDb({ account, currentBinding: null, paymentRecord: { accountId: 'acc_other' } });
    await expect(run(db, { amount: 29900, status: 'captured' })).rejects.toThrow(/already been used/);
  });

  it('allows the same account to re-activate with its own payment', async () => {
    const db = makeDb({ account, currentBinding: null, paymentRecord: { accountId: 'acc_1' } });
    const result = await run(db, { amount: 29900, status: 'captured' });
    expect(result.status).toBe('activated');
    expect(db.paymentProviderRecord.create).not.toHaveBeenCalled();
  });

  it('records the payment on first use', async () => {
    const db = makeDb({ account, currentBinding: null });
    await run(db, { amount: 29900, status: 'captured' });
    expect(db.paymentProviderRecord.create).toHaveBeenCalledWith({
      data: { accountId: 'acc_1', razorpayPaymentId: PAYMENT_ID },
    });
  });
});
