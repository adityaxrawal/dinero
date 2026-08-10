/**
 * Reconciles local subscription state against Razorpay's records.
 *
 * A safety net for missed webhooks. Webhook delivery can fail permanently -- an
 * outage, a deploy at the wrong moment, exhausted retries -- and the result is a
 * subscription frozen in a stale state: a user who cancelled still being billed
 * as active, or one who paid still locked out.
 *
 * This job periodically asks Razorpay for the true status of each subscription
 * and corrects any divergence, so the system is eventually consistent with the
 * payment provider regardless of what was delivered.
 */
import type { PrismaClient } from '@prisma/client';
import { logAuditEvent, type AuditWriter } from '../lib/audit';
import type { RazorpaySubscriptions } from '../lib/razorpay';

const RAZORPAY_TO_LOCAL_STATUS: Record<string, string> = {
  active: 'active',
  halted: 'past_due',
  cancelled: 'canceled',
  completed: 'canceled',
};

export interface ReconciliationSummary {
  checked: number;
  drifted: number;
  corrected: number;
}

export type ReconciliationDb = {
  subscription: Pick<PrismaClient['subscription'], 'findMany' | 'update'>;
  paymentProviderRecord: Pick<PrismaClient['paymentProviderRecord'], 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

/**
 * Reconciles local subscription state against Razorpay's records.
 *
 * The safety net for permanently failed webhooks, which would otherwise leave a
 * subscription frozen -- a cancelled user still billed as active, or a paying one
 * still locked out.
 */
export async function runBillingReconciliation(
  db: ReconciliationDb,
  razorpay: RazorpaySubscriptions
): Promise<ReconciliationSummary> {
  const subscriptions = await db.subscription.findMany({
    where: { status: { in: ['active', 'past_due'] } },
  });
  let drifted = 0;
  let corrected = 0;

  for (const sub of subscriptions as unknown as {
    id: string;
    accountId: string;
    status: string;
  }[]) {
    const record = await db.paymentProviderRecord.findFirst({
      where: { subscriptionId: sub.id },
      orderBy: { createdAt: 'desc' },
    });
    if (!record?.razorpaySubscriptionId) continue;

    let remote;
    try {
      remote = await razorpay.fetch(record.razorpaySubscriptionId);
    } catch {
      continue;
    }

    const expectedLocalStatus = RAZORPAY_TO_LOCAL_STATUS[remote.status];
    if (!expectedLocalStatus || expectedLocalStatus === sub.status) continue;

    drifted++;
    await db.subscription.update({ where: { id: sub.id }, data: { status: expectedLocalStatus } });
    await logAuditEvent(db.licensingAuditLog, {
      accountId: sub.accountId,
      eventType: 'billing_reconciliation_correction',
      payload: {
        subscription_id: sub.id,
        local_status_was: sub.status,
        corrected_to: expectedLocalStatus,
        razorpay_status: remote.status,
      },
    });
    corrected++;
  }

  return { checked: subscriptions.length, drifted, corrected };
}

