import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-BILL-005: POST /api/billing/cancel
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface CancelInput {
  account_id: string;
  cancel_at_period_end?: boolean; // defaults to true
}

export interface CancelResult {
  status: 'cancelled' | 'reactivated';
  cancel_at_period_end: boolean;
}

export type CancelDb = {
  subscription: Pick<PrismaClient['subscription'], 'findFirst' | 'update'>;
  licensingAuditLog: AuditWriter;
};

export async function cancelSubscription(db: CancelDb, input: CancelInput): Promise<CancelResult> {
  const subscription = await db.subscription.findFirst({ where: { accountId: input.account_id }, orderBy: { createdAt: 'desc' } });
  if (!subscription) {
    throw new LicensingApiError('NOT_FOUND', 'No subscription found for this account');
  }

  const cancelAtPeriodEnd = input.cancel_at_period_end ?? true;

  // Doc 30 TASK-BILL-005: "Cancellation never triggers data deletion" -- this
  // function's signature is structurally incapable of it (no data-deletion
  // handle at all); a cancelled-but-not-deleted account simply transitions
  // to LOCKED (read-only) at period end, a status the desktop already
  // computes from `status='canceled'` + an elapsed `current_period_end`.
  await db.subscription.update({
    where: { id: subscription.id },
    data: { cancelAtPeriodEnd, status: cancelAtPeriodEnd ? subscription.status : 'canceled' },
  });

  await logAuditEvent(db.licensingAuditLog, {
    accountId: input.account_id,
    eventType: 'subscription_cancelled',
    payload: { cancel_at_period_end: cancelAtPeriodEnd },
  });

  return { status: 'cancelled', cancel_at_period_end: cancelAtPeriodEnd };
}

/// Doc 30 TASK-BILL-005: "a 'Reactivate' button if still within the
/// cancelled-but-active period" -- undoes cancel_at_period_end before the
/// period actually ends.
export async function reactivateSubscription(db: CancelDb, accountId: string): Promise<CancelResult> {
  const subscription = await db.subscription.findFirst({ where: { accountId }, orderBy: { createdAt: 'desc' } });
  if (!subscription) {
    throw new LicensingApiError('NOT_FOUND', 'No subscription found for this account');
  }
  if (subscription.currentPeriodEnd && new Date(subscription.currentPeriodEnd as unknown as string) < new Date()) {
    throw new LicensingApiError('VALIDATION_ERROR', 'Subscription period has already ended -- reactivation requires a new checkout');
  }

  await db.subscription.update({ where: { id: subscription.id }, data: { cancelAtPeriodEnd: false } });

  await logAuditEvent(db.licensingAuditLog, { accountId, eventType: 'subscription_reactivated' });

  return { status: 'reactivated', cancel_at_period_end: false };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return;
  }
  const { account_id, cancel_at_period_end, reactivate } = req.body ?? {};
  if (!account_id) {
    res.status(400).json({ code: 'VALIDATION_ERROR', message: 'account_id is required' });
    return;
  }
  try {
    const result = reactivate
      ? await reactivateSubscription(prisma, account_id)
      : await cancelSubscription(prisma, { account_id, cancel_at_period_end });
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}

export default withRequestLogging('billing/cancel', handler);
