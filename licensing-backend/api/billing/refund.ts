import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-BILL-006: internal admin-operated endpoint. No self-service
// refund UI in v1.0 (the 14-day trial already de-risks purchase, Doc 03).
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields } from '../../lib/api_helpers';
import { assertAdminAuthorized } from '../../lib/admin_auth';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';
import { consoleEmailSender, type EmailSender } from '../../lib/email';
import { realRazorpayRefunds, type RazorpayRefunds } from '../../lib/razorpay';

export interface RefundInput {
  account_id: string;
  reason: string;
}

export interface RefundResult {
  status: 'refunded';
}

export type RefundDb = {
  account: Pick<PrismaClient['account'], 'findUnique'>;
  subscription: Pick<PrismaClient['subscription'], 'findFirst' | 'update'>;
  paymentProviderRecord: Pick<PrismaClient['paymentProviderRecord'], 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

export async function refundAccount(
  db: RefundDb,
  input: RefundInput,
  razorpay: RazorpayRefunds,
  emailSender: EmailSender = consoleEmailSender
): Promise<RefundResult> {
  const account = await db.account.findUnique({ where: { id: input.account_id } });
  if (!account) {
    throw new LicensingApiError('NOT_FOUND', 'Unknown account');
  }
  const subscription = await db.subscription.findFirst({ where: { accountId: input.account_id }, orderBy: { createdAt: 'desc' } });
  if (!subscription) {
    throw new LicensingApiError('NOT_FOUND', 'No subscription found for this account');
  }
  const paymentRecord = await db.paymentProviderRecord.findFirst({
    where: { accountId: input.account_id, razorpayPaymentId: { not: null } },
    orderBy: { createdAt: 'desc' },
  });
  if (!paymentRecord?.razorpayPaymentId) {
    throw new LicensingApiError('NOT_FOUND', 'No successful charge found to refund');
  }

  await razorpay.create(paymentRecord.razorpayPaymentId);

  await db.subscription.update({ where: { id: subscription.id }, data: { status: 'canceled' } });

  await logAuditEvent(db.licensingAuditLog, {
    accountId: input.account_id,
    eventType: 'refund_issued',
    payload: { reason: input.reason, razorpay_payment_id: paymentRecord.razorpayPaymentId },
  });

  await emailSender.send({
    to: account.email,
    subject: 'Your Dinero subscription has been refunded',
    body: `Your most recent charge has been refunded and your subscription is now cancelled. Reason: ${input.reason}`,
  });

  return { status: 'refunded' };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  if (!requirePostWithFields(req, res, ['account_id', 'reason'])) return;
  try {
    assertAdminAuthorized(req.headers.authorization);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    throw e;
  }
  const { account_id, reason } = req.body;
  const keyId = process.env.RAZORPAY_KEY_ID;
  const keySecret = process.env.RAZORPAY_KEY_SECRET;
  if (!keyId || !keySecret) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await refundAccount(prisma, { account_id, reason }, realRazorpayRefunds(keyId, keySecret));
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}

export default withRequestLogging('billing/refund', handler);
