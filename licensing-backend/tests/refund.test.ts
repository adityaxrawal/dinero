// Doc 30 TASK-BILL-006 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { refundAccount, type RefundDb } from '../api/billing/refund';
import { assertAdminAuthorized } from '../lib/admin_auth';

function makeDb(): RefundDb {
  return {
    account: { findUnique: vi.fn().mockResolvedValue({ id: 'acc_1', email: 'user@example.com' }) } as unknown as RefundDb['account'],
    subscription: {
      findFirst: vi.fn().mockResolvedValue({ id: 'sub_1', accountId: 'acc_1', status: 'active' }),
      update: vi.fn().mockResolvedValue({}),
    } as unknown as RefundDb['subscription'],
    paymentProviderRecord: {
      findFirst: vi.fn().mockResolvedValue({ razorpayPaymentId: 'pay_123' }),
    } as unknown as RefundDb['paymentProviderRecord'],
    licensingAuditLog: { create: vi.fn().mockResolvedValue({}), findMany: vi.fn().mockResolvedValue([]) } as unknown as RefundDb['licensingAuditLog'],
  };
}

describe('test_refund_cancels_subscription', () => {
  it('issues the Razorpay refund and cancels the subscription', async () => {
    const db = makeDb();
    const razorpay = { create: vi.fn().mockResolvedValue({ id: 'rfnd_1', status: 'processed' }) };
    const result = await refundAccount(db, { account_id: 'acc_1', reason: 'customer request' }, razorpay);
    expect(result.status).toBe('refunded');
    expect(razorpay.create).toHaveBeenCalledWith('pay_123');
    expect(db.subscription.update).toHaveBeenCalledWith(expect.objectContaining({ data: { status: 'canceled' } }));
  });
});

describe('test_refund_logged_to_audit', () => {
  it('logs the reason and payment id', async () => {
    const db = makeDb();
    const razorpay = { create: vi.fn().mockResolvedValue({ id: 'rfnd_1', status: 'processed' }) };
    await refundAccount(db, { account_id: 'acc_1', reason: 'customer request' }, razorpay);
    expect(db.licensingAuditLog.create).toHaveBeenCalledWith(
      expect.objectContaining({ data: expect.objectContaining({ eventType: 'refund_issued', payload: expect.objectContaining({ reason: 'customer request' }) }) })
    );
  });
});

describe('test_refund_requires_admin_auth', () => {
  it('rejects without a matching bearer token', () => {
    process.env.ADMIN_API_TOKEN = 'secret-token';
    expect(() => assertAdminAuthorized(undefined)).toThrow();
    expect(() => assertAdminAuthorized('Bearer wrong-token')).toThrow();
    expect(() => assertAdminAuthorized('Bearer secret-token')).not.toThrow();
    delete process.env.ADMIN_API_TOKEN;
  });
});
