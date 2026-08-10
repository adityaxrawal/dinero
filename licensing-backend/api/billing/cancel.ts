/**
 * Subscription cancellation and its reversal.
 *
 * Cancellation is scheduled rather than immediate: the subscription is marked
 * to end at the close of the paid period, so a user who has already paid keeps
 * what they bought. That also makes reactivation possible -- while the period
 * is still running the subscription is merely flagged, and clearing the flag
 * restores it without a fresh payment.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface CancelInput {
  account_id: string;
  cancel_at_period_end?: boolean;
}

export interface CancelResult {
  status: 'cancelled' | 'reactivated';
  cancel_at_period_end: boolean;
}

export type CancelDb = {
  subscription: Pick<PrismaClient['subscription'], 'findFirst' | 'update'>;
  licensingAuditLog: AuditWriter;
};

/**
 * Schedule cancellation at the end of the current paid period.
 */
export async function cancelSubscription(db: CancelDb, input: CancelInput): Promise<CancelResult> {
  const subscription = await db.subscription.findFirst({
    where: { accountId: input.account_id },
    orderBy: { createdAt: 'desc' },
  });
  if (!subscription) {
    throw new LicensingApiError('NOT_FOUND', 'No subscription found for this account');
  }

  const cancelAtPeriodEnd = input.cancel_at_period_end ?? true;

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

/**
 * Undo a scheduled cancellation, provided the period has not yet ended.
 *
 * Once it has, there is nothing to reactivate and the user must subscribe
 * afresh -- which is what the status guard below enforces.
 */
export async function reactivateSubscription(
  db: CancelDb,
  accountId: string
): Promise<CancelResult> {
  const subscription = await db.subscription.findFirst({
    where: { accountId },
    orderBy: { createdAt: 'desc' },
  });
  if (!subscription) {
    throw new LicensingApiError('NOT_FOUND', 'No subscription found for this account');
  }
  if (
    subscription.currentPeriodEnd &&
    new Date(subscription.currentPeriodEnd as unknown as string) < new Date()
  ) {
    throw new LicensingApiError(
      'VALIDATION_ERROR',
      'Subscription period has already ended -- reactivation requires a new checkout'
    );
  }

  await db.subscription.update({
    where: { id: subscription.id },
    data: { cancelAtPeriodEnd: false },
  });

  await logAuditEvent(db.licensingAuditLog, { accountId, eventType: 'subscription_reactivated' });

  return { status: 'reactivated', cancel_at_period_end: false };
}

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
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
    sendApiError(res, e);
  }
}

export default withRequestLogging('billing/cancel', handler);
