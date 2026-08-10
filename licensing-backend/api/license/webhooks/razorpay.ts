/**
 * Razorpay webhook receiver: the authoritative feed of billing state changes.
 *
 * This is how the service learns about events that happen outside any user
 * session -- a renewal charging successfully, a card failing, a subscription
 * lapsing. Two properties matter above all:
 *
 *   - Signature verification. The handler is a public URL, so a forged request
 *     could otherwise fabricate a paid subscription. The HMAC is computed over
 *     the raw body, before any parse-and-reserialise could alter the bytes.
 *   - Idempotency. Razorpay retries until it receives a success, so the same
 *     event id will arrive more than once. Events are recorded and duplicates
 *     short-circuit, otherwise a retried renewal would extend a period twice.
 *
 * Event types are dispatched through a lookup table; an unrecognised event is
 * acknowledged and ignored rather than erroring, so Razorpay does not retry
 * something this service has simply chosen not to act on.
 */
import { withRequestLogging } from '../../../lib/request_logging';
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../../lib/db';
import { verifyWebhookSignature } from '../../../lib/razorpay';
import { logAuditEvent, countRecentEvents, type AuditWriter } from '../../../lib/audit';

export interface RazorpayWebhookEvent {
  id: string;
  event: 'subscription.created' | 'invoice.paid' | 'invoice.payment_failed' | string;
  payload: {
    subscription?: { entity?: { id?: string; notes?: { account_id?: string } } };
    invoice?: { entity?: { subscription_id?: string; billing_end?: number } };
  };
}

export type WebhookDb = {
  subscription: Pick<PrismaClient['subscription'], 'findFirst' | 'update' | 'create'>;
  paymentProviderRecord: Pick<PrismaClient['paymentProviderRecord'], 'create' | 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

/**
 * Resolves a Razorpay subscription id to the local subscription.
 */
async function findSubscriptionByRazorpaySubscriptionId(
  db: WebhookDb,
  razorpaySubscriptionId: string
) {
  const record = await db.paymentProviderRecord.findFirst({
    where: { razorpaySubscriptionId },
    orderBy: { createdAt: 'desc' },
  });
  if (!record?.subscriptionId) return null;
  return db.subscription.findFirst({ where: { id: record.subscriptionId } });
}

/**
 * Whether this event id has already been processed.
 *
 * Razorpay retries until it receives a success, so the same event will arrive
 * more than once -- without this a retried renewal would extend a period twice.
 */
async function isDuplicateEvent(
  db: Pick<WebhookDb, 'licensingAuditLog'>,
  eventId: string
): Promise<boolean> {
  const count = await countRecentEvents(
    db.licensingAuditLog,
    'webhook_processed',
    Number.MAX_SAFE_INTEGER,
    (p) => (p as { event_id?: string } | null)?.event_id === eventId
  );
  return count > 0;
}

/**
 * Binds a new Razorpay subscription to the account and heals any local row.
 */
async function onSubscriptionCreated(db: WebhookDb, event: RazorpayWebhookEvent): Promise<void> {
  const accountId = event.payload.subscription?.entity?.notes?.account_id;
  const razorpaySubscriptionId = event.payload.subscription?.entity?.id;
  if (!accountId) return;

  const existing = await db.subscription.findFirst({
    where: { accountId },
    orderBy: { createdAt: 'desc' },
  });
  if (existing) {
    await db.subscription.update({ where: { id: existing.id }, data: { status: 'active' } });
  }
  await db.paymentProviderRecord.create({
    data: { accountId, subscriptionId: existing?.id, razorpaySubscriptionId },
  });
}

/**
 * Extends the paid period following a successful charge.
 */
async function onInvoicePaid(db: WebhookDb, event: RazorpayWebhookEvent): Promise<void> {
  const razorpaySubscriptionId = event.payload.invoice?.entity?.subscription_id;
  const billingEndUnix = event.payload.invoice?.entity?.billing_end;
  if (!razorpaySubscriptionId || !billingEndUnix) return;

  const subscription = await findSubscriptionByRazorpaySubscriptionId(db, razorpaySubscriptionId);
  if (!subscription) return;

  await db.subscription.update({
    where: { id: subscription.id },
    data: { currentPeriodEnd: new Date(billingEndUnix * 1000), status: 'active' },
  });
}

/**
 * Moves the subscription into the grace period after a failed charge.
 *
 * Grace rather than immediate lockout, so a card that failed does not instantly
 * destroy access for a paying customer.
 */
async function onPaymentFailed(db: WebhookDb, event: RazorpayWebhookEvent): Promise<void> {
  const razorpaySubscriptionId = event.payload.invoice?.entity?.subscription_id;
  if (!razorpaySubscriptionId) return;

  const subscription = await findSubscriptionByRazorpaySubscriptionId(db, razorpaySubscriptionId);
  if (!subscription) return;

  await db.subscription.update({ where: { id: subscription.id }, data: { status: 'past_due' } });
}

// Event type to handler. A table rather than a switch so that an unhandled
// event type is simply absent, and the dispatcher can no-op cleanly.
const EVENT_HANDLERS: Record<
  string,
  (db: WebhookDb, event: RazorpayWebhookEvent) => Promise<void>
> = {
  'subscription.created': onSubscriptionCreated,
  'invoice.paid': onInvoicePaid,
  'invoice.payment_failed': onPaymentFailed,
};

/**
 * Dispatch one verified webhook event, skipping duplicates.
 */
export async function processWebhookEvent(
  db: WebhookDb,
  event: RazorpayWebhookEvent
): Promise<{ status: 'processed' | 'duplicate_ignored' }> {
  if (await isDuplicateEvent(db, event.id)) {
    return { status: 'duplicate_ignored' };
  }

  await EVENT_HANDLERS[event.event]?.(db, event);

  await logAuditEvent(db.licensingAuditLog, {
    eventType: 'webhook_processed',
    payload: { event_id: event.id, event_type: event.event },
  });

  return { status: 'processed' };
}

/**
 * Verifies the signature over the raw body, then dispatches the event.
 *
 * The HMAC is computed before any parse-and-reserialise, which could reorder keys
 * or alter whitespace and invalidate a legitimate signature.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return;
  }
  const webhookSecret = process.env.RAZORPAY_WEBHOOK_SECRET;
  const signature = req.headers['x-razorpay-signature'] as string | undefined;
  if (!webhookSecret || !signature) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  const rawBody = JSON.stringify(req.body);
  if (!verifyWebhookSignature(rawBody, signature, webhookSecret)) {
    res.status(400).json({
      code: 'INVALID_WEBHOOK_SIGNATURE',
      message: 'Webhook signature verification failed',
    });
    return;
  }
  try {
    const result = await processWebhookEvent(prisma, req.body as RazorpayWebhookEvent);
    res.status(200).json(result);
  } catch {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}

export default withRequestLogging('license/webhooks/razorpay', handler);
