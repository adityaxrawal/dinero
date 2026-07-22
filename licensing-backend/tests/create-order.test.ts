// Doc 30 TASK-BILL-002 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { createOrder, type CreateOrderDb } from '../api/billing/create-order';
import { verifyPaymentSignature } from '../lib/razorpay';
import { createHmac } from 'node:crypto';

function makeDb(plan: Record<string, unknown> | null, account: Record<string, unknown> | null): CreateOrderDb {
  return {
    plan: { findUnique: vi.fn().mockResolvedValue(plan) } as unknown as CreateOrderDb['plan'],
    account: {
      findUnique: vi.fn().mockResolvedValue(account),
      create: vi.fn().mockResolvedValue({ id: 'acc_new', email: 'user@example.com' }),
    } as unknown as CreateOrderDb['account'],
  };
}

describe('test_order_creation_returns_valid_config', () => {
  it('returns order id/amount/currency/key_id for an active plan', async () => {
    const db = makeDb({ id: 'desktop_pro_monthly', isActive: true, amountMinor: 29900, currency: 'INR' }, { id: 'acc_1', email: 'user@example.com' });
    const razorpay = { create: vi.fn().mockResolvedValue({ id: 'order_abc', amount: 29900, currency: 'INR' }) };
    const result = await createOrder(db, razorpay, { email: 'user@example.com', plan_id: 'desktop_pro_monthly' }, 'rzp_test_key');
    expect(result).toEqual({ order_id: 'order_abc', amount: 29900, currency: 'INR', key_id: 'rzp_test_key' });
  });

  it('finds-or-creates the account when this is a first-time purchaser', async () => {
    const db = makeDb({ id: 'desktop_pro_monthly', isActive: true, amountMinor: 29900, currency: 'INR' }, null);
    const razorpay = { create: vi.fn().mockResolvedValue({ id: 'order_abc', amount: 29900, currency: 'INR' }) };
    await createOrder(db, razorpay, { email: 'new@example.com', plan_id: 'desktop_pro_monthly' }, 'rzp_test_key');
    expect(db.account.create).toHaveBeenCalled();
  });

  it('rejects an inactive plan', async () => {
    const db = makeDb({ id: 'x', isActive: false, amountMinor: 1, currency: 'INR' }, { id: 'acc_1', email: 'user@example.com' });
    const razorpay = { create: vi.fn() };
    await expect(createOrder(db, razorpay, { email: 'user@example.com', plan_id: 'x' }, 'rzp_test_key')).rejects.toMatchObject({ code: 'NOT_FOUND' });
  });
});

describe('test_client_side_success_claim_rejected_without_valid_signature', () => {
  const keySecret = 'test-webhook-secret';
  it('accepts a genuinely valid signature', () => {
    const orderId = 'order_abc';
    const paymentId = 'pay_123';
    const validSignature = createHmac('sha256', keySecret).update(`${orderId}|${paymentId}`).digest('hex');
    expect(verifyPaymentSignature(orderId, paymentId, validSignature, keySecret)).toBe(true);
  });

  it('rejects a forged/client-supplied signature that does not match', () => {
    const orderId = 'order_abc';
    const paymentId = 'pay_123';
    const forgedSignature = 'deadbeef'.repeat(8); // well-formed hex, wrong value
    expect(verifyPaymentSignature(orderId, paymentId, forgedSignature, keySecret)).toBe(false);
  });

  it('rejects a signature computed with the wrong secret', () => {
    const orderId = 'order_abc';
    const paymentId = 'pay_123';
    const wrongSecretSignature = createHmac('sha256', 'not-the-real-secret').update(`${orderId}|${paymentId}`).digest('hex');
    expect(verifyPaymentSignature(orderId, paymentId, wrongSecretSignature, keySecret)).toBe(false);
  });
});
