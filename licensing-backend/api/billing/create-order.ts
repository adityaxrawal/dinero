/**
 * Creates a Razorpay order, the first step of a purchase.
 *
 * The amount is read from the plan record server-side and never taken from the
 * request. That is the essential property: a client-supplied price would let a
 * caller order the paid plan for a rupee. The email is attached to the order's
 * notes so the subsequent activation can be tied back to an account.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma, findOrCreateAccount } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields } from '../../lib/api_helpers';
import {
  realRazorpayOrders,
  getRazorpayCredentials,
  type RazorpayOrders,
} from '../../lib/razorpay';

export interface CreateOrderInput {
  email: string;
  plan_id: string;
}

export interface CreateOrderResult {
  order_id: string;
  amount: number;
  currency: string;
  key_id: string;
}

export type CreateOrderDb = {
  plan: Pick<PrismaClient['plan'], 'findUnique'>;
  account: Pick<PrismaClient['account'], 'findUnique' | 'create'>;
};

/**
 * Creates a Razorpay order for a plan.
 *
 * The amount is read from the plan record server-side and never taken from the
 * request -- a client-supplied price would let a caller buy the paid plan for a
 * rupee.
 */
export async function createOrder(
  db: CreateOrderDb,
  razorpay: RazorpayOrders,
  input: CreateOrderInput,
  razorpayKeyId: string
): Promise<CreateOrderResult> {
  const plan = await db.plan.findUnique({ where: { id: input.plan_id } });
  // Inactive plans are refused as well as unknown ones -- a retired plan id
  // must not remain purchasable just because the row still exists.
  if (!plan || !plan.isActive) {
    throw new LicensingApiError('NOT_FOUND', 'Unknown or inactive plan');
  }

  const account = await findOrCreateAccount(db, input.email);

  const order = await razorpay.create({
    amount: plan.amountMinor,
    currency: plan.currency,
    notes: { account_id: account.id, plan_id: input.plan_id },
  });

  return {
    order_id: order.id,
    amount: order.amount,
    currency: order.currency,
    key_id: razorpayKeyId,
  };
}

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  if (!requirePostWithFields(req, res, ['email', 'plan_id'])) return;
  const { email, plan_id } = req.body;
  const credentials = getRazorpayCredentials();
  if (!credentials) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  const { keyId, keySecret } = credentials;
  try {
    const result = await createOrder(
      prisma,
      realRazorpayOrders(keyId, keySecret),
      { email, plan_id },
      keyId
    );
    res.status(200).json(result);
  } catch (e) {
    sendApiError(res, e);
  }
}

export default withRequestLogging('billing/create-order', handler);
