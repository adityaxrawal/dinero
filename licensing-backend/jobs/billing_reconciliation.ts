// Doc 30 TASK-BILL-008: monthly scheduled job (Vercel Cron) cross-referencing
// every active/past_due local subscription against Razorpay's authoritative
// state, correcting drift (a missed webhook leaving local status stale).
// Backstop for "if webhooks fail silently, users may be incorrectly LOCKED"
// beyond what the user's manual "Refresh License" already covers.
import type { PrismaClient } from '@prisma/client';
import { logAuditEvent, type AuditWriter } from '../lib/audit';
import type { RazorpaySubscriptions } from '../lib/razorpay';

/// Razorpay's subscription-status vocabulary mapped to ours. Anything
/// unrecognized is left untouched rather than guessed.
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
      continue; // Razorpay API unreachable this cycle -- skip, don't guess.
    }

    const expectedLocalStatus = RAZORPAY_TO_LOCAL_STATUS[remote.status];
    if (!expectedLocalStatus || expectedLocalStatus === sub.status) continue;

    drifted++;
    await db.subscription.update({ where: { id: sub.id }, data: { status: expectedLocalStatus } });
    // Doc 30 TASK-BILL-008: "Any detected drift is auto-corrected... and
    // logged... flagged for review, never applied silently without a trace."
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

// Vercel Cron entrypoint lives at api/cron/billing-reconciliation.ts (Vercel
// Cron triggers an HTTP path under api/, not an arbitrary module export) --
// it imports runBillingReconciliation from here.
