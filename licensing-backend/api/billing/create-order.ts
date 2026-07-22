// Doc 30 TASK-BILL-002: POST /api/billing/create-order
//
// Keyed on `email`, not `account_id` (adapted during TASK-BILL-002's larger
// activate-model correction, see Doc 30 changelog): the desktop app has no
// server-side account_id to send at checkout time for a brand-new
// subscriber (an `accounts` row is only ever find-or-created at activation,
// mirroring activate.ts's own pattern) -- create-order does the same
// find-or-create so a first-time purchaser never needs one in advance.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { realRazorpayOrders, type RazorpayOrders } from '../../lib/razorpay';

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

export async function createOrder(
  db: CreateOrderDb,
  razorpay: RazorpayOrders,
  input: CreateOrderInput,
  razorpayKeyId: string
): Promise<CreateOrderResult> {
  const plan = await db.plan.findUnique({ where: { id: input.plan_id } });
  if (!plan || !plan.isActive) {
    throw new LicensingApiError('NOT_FOUND', 'Unknown or inactive plan');
  }

  let account = await db.account.findUnique({ where: { email: input.email } });
  if (!account) {
    account = await db.account.create({ data: { email: input.email } });
  }

  const order = await razorpay.create({
    amount: plan.amountMinor,
    currency: plan.currency,
    notes: { account_id: account.id, plan_id: input.plan_id },
  });

  return { order_id: order.id, amount: order.amount, currency: order.currency, key_id: razorpayKeyId };
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return;
  }
  const { email, plan_id } = req.body ?? {};
  if (!email || !plan_id) {
    res.status(400).json({ code: 'VALIDATION_ERROR', message: 'email and plan_id are required' });
    return;
  }
  const keyId = process.env.RAZORPAY_KEY_ID;
  const keySecret = process.env.RAZORPAY_KEY_SECRET;
  if (!keyId || !keySecret) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await createOrder(prisma, realRazorpayOrders(keyId, keySecret), { email, plan_id }, keyId);
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}
