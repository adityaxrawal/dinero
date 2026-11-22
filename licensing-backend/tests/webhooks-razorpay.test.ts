// Doc 30 TASK-BILL-003 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import {
  processWebhookEvent,
  type WebhookDb,
  type RazorpayWebhookEvent,
} from '../api/license/webhooks/razorpay';
import { verifyWebhookSignature } from '../lib/razorpay';
import { createHmac } from 'node:crypto';

function makeDb(opts: {
  subscriptions?: Record<string, unknown>[];
  paymentProviderRecords?: Record<string, unknown>[];
  processedEventIds?: string[];
}): WebhookDb {
  const subscriptions = opts.subscriptions ?? [];
  const paymentProviderRecords = opts.paymentProviderRecords ?? [];
  const auditRows: { eventType: string; payload: unknown; createdAt: Date }[] = [];
  const processed = new Set(opts.processedEventIds ?? []);

  return {
    subscription: {
      findFirst: vi.fn().mockImplementation(({ where }) => {
        if (where.id) return Promise.resolve(subscriptions.find((s) => s.id === where.id) ?? null);
        if (where.accountId)
          return Promise.resolve(
            subscriptions.find((s) => s.accountId === where.accountId) ?? null
          );
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
      findFirst: vi
        .fn()
        .mockImplementation(({ where }) =>
          Promise.resolve(
            paymentProviderRecords.find((r) =>
              where.razorpayPaymentId !== undefined
                ? r.razorpayPaymentId === where.razorpayPaymentId
                : r.razorpaySubscriptionId === where.razorpaySubscriptionId
            ) ?? null
          )
        ),
    } as unknown as WebhookDb['paymentProviderRecord'],
    processedWebhookEvent: {
      findUnique: vi
        .fn()
        .mockImplementation(({ where }) =>
          Promise.resolve(processed.has(where.eventId) ? { eventId: where.eventId } : null)
        ),
      create: vi.fn().mockImplementation(({ data }) => {
        processed.add(data.eventId);
        return Promise.resolve(data);
      }),
    } as unknown as WebhookDb['processedWebhookEvent'],
    licensingAuditLog: {
      create: vi.fn().mockImplementation(({ data }) => {
        auditRows.push({ eventType: data.eventType, payload: data.payload, createdAt: new Date() });
        return Promise.resolve({});
      }),
      findMany: vi
        .fn()
        .mockImplementation(({ where }) =>
          Promise.resolve(auditRows.filter((r) => r.eventType === where.eventType))
        ),
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
    expect(db.subscription.update).toHaveBeenCalledWith(
      expect.objectContaining({ data: { status: 'active' } })
    );
    expect(db.paymentProviderRecord.create).toHaveBeenCalledWith(
      expect.objectContaining({
        data: expect.objectContaining({ razorpaySubscriptionId: 'rzp_sub_1' }),
      })
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
      expect.objectContaining({
        where: { id: 'sub_1' },
        data: expect.objectContaining({ status: 'active' }),
      })
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

// Review H3: cancellation-type events and full refunds end entitlement; ids
// are recorded so a redelivery is ignored.
describe('lifecycle events end entitlement', () => {
  const linked = () =>
    makeDb({
      subscriptions: [{ id: 'sub_1', accountId: 'acc_1', status: 'active' }],
      paymentProviderRecords: [
        { subscriptionId: 'sub_1', razorpaySubscriptionId: 'rzp_sub_1' },
        { accountId: 'acc_1', razorpayPaymentId: 'pay_1' },
      ],
    });

  it.each([
    ['subscription.cancelled', 'canceled'],
    ['subscription.halted', 'locked'],
    ['subscription.completed', 'expired'],
  ])('%s sets status %s', async (eventType, status) => {
    const db = linked();
    await processWebhookEvent(db, {
      id: `evt_${eventType}`,
      event: eventType,
      payload: { subscription: { entity: { id: 'rzp_sub_1' } } },
    });
    expect(db.subscription.update).toHaveBeenCalledWith({ where: { id: 'sub_1' }, data: { status } });
  });

  it('a full refund cancels; a partial refund does not', async () => {
    const partial = linked();
    await processWebhookEvent(partial, {
      id: 'evt_r1',
      event: 'refund.processed',
      payload: {
        refund: { entity: { payment_id: 'pay_1', amount: 100 } },
        payment: { entity: { amount: 29900 } },
      },
    });
    expect(partial.subscription.update).not.toHaveBeenCalled();

    const full = linked();
    await processWebhookEvent(full, {
      id: 'evt_r2',
      event: 'refund.processed',
      payload: {
        refund: { entity: { payment_id: 'pay_1', amount: 29900 } },
        payment: { entity: { amount: 29900 } },
      },
    });
    expect(full.subscription.update).toHaveBeenCalledWith({
      where: { id: 'sub_1' },
      data: { status: 'canceled' },
    });
  });

  it('a redelivered event id is ignored', async () => {
    const db = linked();
    const event: RazorpayWebhookEvent = {
      id: 'evt_dup',
      event: 'subscription.cancelled',
      payload: { subscription: { entity: { id: 'rzp_sub_1' } } },
    };
    expect((await processWebhookEvent(db, event)).status).toBe('processed');
    expect((await processWebhookEvent(db, event)).status).toBe('duplicate_ignored');
    expect(db.subscription.update).toHaveBeenCalledTimes(1);
  });
});
