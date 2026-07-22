import { withRequestLogging } from '../../../lib/request_logging';
// Doc 30 TASK-BILL-003: POST /api/license/webhooks/razorpay
// Resolves Document 21 TPI-OQ-04 / Document 19 §14.5's route inventory entry
// -- this is the one real Razorpay webhook route in the system (TASK-LIC-006
// is a superseded duplicate, see its own note).
//
// Real limitation, flagged not hidden: Razorpay's actual webhook payload is
// deeply nested (event.payload.subscription.entity / .payment.entity /
// .order.entity depending on event type) and this pass hasn't been run
// against a real Razorpay webhook delivery (no live account). `parseEvent`
// below extracts the handful of fields this handler actually needs; treat
// its exact field paths as best-effort until verified against a real
// delivered payload.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../../lib/db';
import { verifyWebhookSignature } from '../../../lib/razorpay';
import { logAuditEvent, countRecentEvents, type AuditWriter } from '../../../lib/audit';

export interface RazorpayWebhookEvent {
  id: string; // Razorpay's event ID -- idempotency key
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

/// Resolves a Razorpay subscription id back to *our* subscription row --
/// every invoice event only ever carries Razorpay's own id, never our
/// internal one, so `payment_provider_records` (written at
/// `subscription.created`) is the only link between the two.
async function findSubscriptionByRazorpaySubscriptionId(db: WebhookDb, razorpaySubscriptionId: string) {
  const record = await db.paymentProviderRecord.findFirst({ where: { razorpaySubscriptionId }, orderBy: { createdAt: 'desc' } });
  if (!record?.subscriptionId) return null;
  return db.subscription.findFirst({ where: { id: record.subscriptionId } });
}

/// Doc 30 TASK-BILL-003: "Idempotent by Razorpay's event ID." Reuses
/// licensing_audit_log rather than a dedicated processed-events table --
/// consistent with how rate-limiting/trial-abuse tracking already reuse it
/// in this codebase.
export async function isDuplicateEvent(db: Pick<WebhookDb, 'licensingAuditLog'>, eventId: string): Promise<boolean> {
  const count = await countRecentEvents(db.licensingAuditLog, 'webhook_processed', Number.MAX_SAFE_INTEGER, (p) => (p as { event_id?: string } | null)?.event_id === eventId);
  return count > 0;
}

export async function processWebhookEvent(db: WebhookDb, event: RazorpayWebhookEvent): Promise<{ status: 'processed' | 'duplicate_ignored' }> {
  if (await isDuplicateEvent(db, event.id)) {
    return { status: 'duplicate_ignored' };
  }

  switch (event.event) {
    case 'subscription.created': {
      const accountId = event.payload.subscription?.entity?.notes?.account_id;
      const razorpaySubscriptionId = event.payload.subscription?.entity?.id;
      if (accountId) {
        const existing = await db.subscription.findFirst({ where: { accountId }, orderBy: { createdAt: 'desc' } });
        if (existing) {
          await db.subscription.update({ where: { id: existing.id }, data: { status: 'active' } });
        }
        await db.paymentProviderRecord.create({
          data: { accountId, subscriptionId: existing?.id, razorpaySubscriptionId },
        });
      }
      break;
    }
    case 'invoice.paid': {
      const razorpaySubscriptionId = event.payload.invoice?.entity?.subscription_id;
      const billingEndUnix = event.payload.invoice?.entity?.billing_end;
      if (razorpaySubscriptionId && billingEndUnix) {
        // Doc 30 TASK-BILL-003: "extend current_period_end, heal past_due -> active."
        const subscription = await findSubscriptionByRazorpaySubscriptionId(db, razorpaySubscriptionId);
        if (subscription) {
          await db.subscription.update({
            where: { id: subscription.id },
            data: { currentPeriodEnd: new Date(billingEndUnix * 1000), status: 'active' },
          });
        }
      }
      break;
    }
    case 'invoice.payment_failed': {
      const razorpaySubscriptionId = event.payload.invoice?.entity?.subscription_id;
      if (razorpaySubscriptionId) {
        // Doc 30 TASK-BILL-003: transitions to past_due -- does NOT lock the
        // desktop app immediately; the local 7-day offline grace (TASK-AUTH-009)
        // handles user-facing degradation. This just reflects accurate
        // status for the next validate/refresh call.
        const subscription = await findSubscriptionByRazorpaySubscriptionId(db, razorpaySubscriptionId);
        if (subscription) {
          await db.subscription.update({ where: { id: subscription.id }, data: { status: 'past_due' } });
        }
      }
      break;
    }
    default:
      break;
  }

  await logAuditEvent(db.licensingAuditLog, {
    eventType: 'webhook_processed',
    payload: { event_id: event.id, event_type: event.event },
  });

  return { status: 'processed' };
}

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
    res.status(400).json({ code: 'INVALID_WEBHOOK_SIGNATURE', message: 'Webhook signature verification failed' });
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
