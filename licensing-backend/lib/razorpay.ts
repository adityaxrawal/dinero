// Doc 30 TASK-BILL-002/003: Razorpay integration. No live Razorpay account
// exists yet (placeholder credentials) -- order creation is behind an
// injectable RazorpayOrders interface so callers/tests never need a real
// network call; signature verification is real HMAC-SHA256 (deterministic,
// no network dependency at all) and works identically against placeholder
// or real keys.
import { createHmac, timingSafeEqual } from 'node:crypto';

export interface RazorpayOrder {
  id: string;
  amount: number;
  currency: string;
}

export interface RazorpayOrders {
  create(params: { amount: number; currency: string; notes?: Record<string, string> }): Promise<RazorpayOrder>;
}

/// Real Razorpay SDK-backed implementation. Constructed lazily so importing
/// this module never requires RAZORPAY_KEY_ID/SECRET to be set (tests never
/// construct this).
export function realRazorpayOrders(keyId: string, keySecret: string): RazorpayOrders {
  return {
    async create(params) {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const Razorpay = require('razorpay');
      const client = new Razorpay({ key_id: keyId, key_secret: keySecret });
      const order = await client.orders.create(params);
      return { id: order.id, amount: order.amount, currency: order.currency };
    },
  };
}

export interface RazorpayPaymentDetails {
  orderId: string;
  amount: number;
  notes?: Record<string, string>;
}

export interface RazorpayPayments {
  /// Doc 19 §14.2/Doc 40 §4: activation is given only `razorpay_payment_id`
  /// (no order_id -- the already-shipped desktop client never sends one).
  /// Razorpay's own Fetch Payment API always returns the payment's order_id,
  /// which is what makes verifying the signature possible without the
  /// client re-supplying it -- and lets the *same* payment_id/signature pair
  /// be resubmitted indefinitely (e.g. re-activating an existing paid
  /// subscription on a replacement Mac, Doc 18 §12.2) since Razorpay's
  /// signature is a permanent proof of settlement, not a one-time token.
  fetch(paymentId: string): Promise<RazorpayPaymentDetails>;
}

export function realRazorpayPayments(keySecret: string): RazorpayPayments {
  return {
    async fetch(paymentId) {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const Razorpay = require('razorpay');
      const client = new Razorpay({ key_id: process.env.RAZORPAY_KEY_ID, key_secret: keySecret });
      const payment = await client.payments.fetch(paymentId);
      return { orderId: payment.order_id, amount: payment.amount, notes: payment.notes };
    },
  };
}

export interface RazorpayRefunds {
  /// Doc 30 TASK-BILL-006: refunds the most recent successful charge for an
  /// admin-operated support request.
  create(paymentId: string): Promise<{ id: string; status: string }>;
}

export function realRazorpayRefunds(keyId: string, keySecret: string): RazorpayRefunds {
  return {
    async create(paymentId) {
      // eslint-disable-next-line @typescript-eslint/no-var-requires
      const Razorpay = require('razorpay');
      const client = new Razorpay({ key_id: keyId, key_secret: keySecret });
      const refund = await client.payments.refund(paymentId, {});
      return { id: refund.id, status: refund.status };
    },
  };
}

/// Razorpay's documented signature scheme: HMAC-SHA256(order_id + "|" +
/// payment_id, key_secret), hex-encoded. Constant-time compare against
/// timing attacks.
export function verifyPaymentSignature(orderId: string, paymentId: string, signature: string, keySecret: string): boolean {
  const expected = createHmac('sha256', keySecret).update(`${orderId}|${paymentId}`).digest('hex');
  const expectedBuf = Buffer.from(expected, 'hex');
  const providedBuf = Buffer.from(signature, 'hex');
  if (expectedBuf.length !== providedBuf.length) return false;
  return timingSafeEqual(expectedBuf, providedBuf);
}

/// Webhook signature scheme (distinct secret from the payment signature,
/// Doc 30 TASK-BILL-003): HMAC-SHA256 of the raw request body.
export function verifyWebhookSignature(rawBody: string, signature: string, webhookSecret: string): boolean {
  const expected = createHmac('sha256', webhookSecret).update(rawBody).digest('hex');
  const expectedBuf = Buffer.from(expected, 'hex');
  const providedBuf = Buffer.from(signature, 'hex');
  if (expectedBuf.length !== providedBuf.length) return false;
  return timingSafeEqual(expectedBuf, providedBuf);
}
