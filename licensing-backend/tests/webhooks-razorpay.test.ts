// Doc 30 TASK-BILL-003 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { processWebhookEvent, type WebhookDb, type RazorpayWebhookEvent } from '../api/license/webhooks/razorpay';
import { verifyWebhookSignature } from '../lib/razorpay';
import { createHmac } from 'node:crypto';

function makeDb(opts: {
  subscriptions?: Record<string, unknown>[];
  paymentProviderRecords?: Record<string, unknown>[];
  processedEventIds?: string[];
}): WebhookDb {
  const subscriptions = opts.subscriptions ?? [];
  const paymentProviderRecords = opts.paymentProviderRecords ?? [];
  const auditRows = (opts.processedEventIds ?? []).map((id) => ({
    eventType: 'webhook_processed',
    payload: { event_id: id },
    createdAt: new Date(),
  }));

  return {
    subscription: {
      findFirst: vi.fn().mockImplementation(({ where }) => {
        if (where.id) return Promise.resolve(subscriptions.find((s) => s.id === where.id) ?? null);
        if (where.accountId) return Promise.resolve(subscriptions.find((s) => s.accountId === where.accountId) ?? null);
        return Promise.resolve(subscriptions[0] ?? null);
      }),
      update: vi.fn().mockImplementation(({ where, data }) => {
        const sub = subscriptions.find((s) => s.id === where.id);
        Object.assign(sub!, data);
        return Promise.resolve(sub);
      }),
      create: vi.fn().mockResolvedValue({}),
    } as unknown as WebhookDb['subscription'],
    paymentProviderRecord: {
      create: vi.fn().mockImplementation(({ data }) => {
        paymentProviderRecords.push(data);
        return Promise.resolve(data);
      }),
      findFirst: vi.fn().mockImplementation(({ where }) =>
        Promise.resolve(paymentProviderRecords.find((r) => r.razorpaySubscriptionId === where.razorpaySubscriptionId) ?? null)
      ),
    } as unknown as WebhookDb['paymentProviderRecord'],
    licensingAuditLog: {
      create: vi.fn().mockImplementation(({ data }) => {
        auditRows.push({ eventType: data.eventType, payload: data.payload, createdAt: new Date() });
        return Promise.resolve({});
      }),
      findMany: vi.fn().mockImplementation(({ where }) => Promise.resolve(auditRows.filter((r) => r.eventType === where.eventType))),
    } as unknown as WebhookDb['licensingAuditLog'],
  };
}

describe('test_subscription_created_creates_row', () => {
  it('marks the existing subscription active and links the Razorpay subscription id', async () => {
    const db = makeDb({ subscriptions: [{ id: 'sub_1', accountId: 'acc_1', status: 'trialing' }] });
    const event: RazorpayWebhookEvent = {
      id: 'evt_1',
      event: 'subscription.created',
      payload: { subscription: { entity: { id: 'rzp_sub_1', notes: { account_id: 'acc_1' } } } },
    };
    const result = await processWebhookEvent(db, event);
    expect(result.status).toBe('processed');
    expect(db.subscription.update).toHaveBeenCalledWith(expect.objectContaining({ data: { status: 'active' } }));
    expect(db.paymentProviderRecord.create).toHaveBeenCalledWith(
      expect.objectContaining({ data: expect.objectContaining({ razorpaySubscriptionId: 'rzp_sub_1' }) })
    );
  });
});

describe('test_invoice_paid_extends_period_and_heals_past_due', () => {
  it('extends current_period_end and flips past_due back to active', async () => {
    const db = makeDb({
      subscriptions: [{ id: 'sub_1', accountId: 'acc_1', status: 'past_due' }],
      paymentProviderRecords: [{ subscriptionId: 'sub_1', razorpaySubscriptionId: 'rzp_sub_1' }],
    });
    const event: RazorpayWebhookEvent = {
      id: 'evt_2',
      event: 'invoice.paid',
      payload: { invoice: { entity: { subscription_id: 'rzp_sub_1', billing_end: 1900000000 } } },
    };
    await processWebhookEvent(db, event);
    expect(db.subscription.update).toHaveBeenCalledWith(
      expect.objectContaining({ where: { id: 'sub_1' }, data: expect.objectContaining({ status: 'active' }) })
    );
  });
});

describe('test_payment_failed_does_not_immediately_lock_backend_side', () => {
  it('transitions to past_due only -- never to locked/canceled', async () => {
    const db = makeDb({
      subscriptions: [{ id: 'sub_1', accountId: 'acc_1', status: 'active' }],
      paymentProviderRecords: [{ subscriptionId: 'sub_1', razorpaySubscriptionId: 'rzp_sub_1' }],
    });
    const event: RazorpayWebhookEvent = {
      id: 'evt_3',
      event: 'invoice.payment_failed',
      payload: { invoice: { entity: { subscription_id: 'rzp_sub_1' } } },
    };
    await processWebhookEvent(db, event);
    expect(db.subscription.update).toHaveBeenCalledWith(
      expect.objectContaining({ data: { status: 'past_due' } })
    );
  });
});

describe('test_duplicate_webhook_event_idempotent', () => {
  it('ignores an event id already processed', async () => {
    const db = makeDb({ processedEventIds: ['evt_1'] });
    const event: RazorpayWebhookEvent = { id: 'evt_1', event: 'subscription.created', payload: {} };
    const result = await processWebhookEvent(db, event);
    expect(result.status).toBe('duplicate_ignored');
    expect(db.subscription.update).not.toHaveBeenCalled();
  });
});

describe('test_invalid_webhook_signature_rejected', () => {
  const webhookSecret = 'test-webhook-secret';
  it('accepts a genuinely valid webhook signature', () => {
    const body = JSON.stringify({ id: 'evt_1' });
    const validSignature = createHmac('sha256', webhookSecret).update(body).digest('hex');
    expect(verifyWebhookSignature(body, validSignature, webhookSecret)).toBe(true);
  });

  it('rejects a forged signature', () => {
    const body = JSON.stringify({ id: 'evt_1' });
    expect(verifyWebhookSignature(body, 'ab'.repeat(32), webhookSecret)).toBe(false);
  });
});
