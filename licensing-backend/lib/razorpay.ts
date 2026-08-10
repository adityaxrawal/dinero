/**
 * Razorpay integration: signature verification and thin API wrappers.
 *
 * The signature helpers are the security-critical part of this file. Razorpay
 * proves authenticity with an HMAC over a known string, and both the payment
 * callback and the webhook are otherwise entirely attacker-controllable input --
 * a forged callback claiming a successful payment would grant a free license.
 *
 * Each API surface is declared as a small interface with a `real*` factory
 * beside it, so tests can substitute a fake without reaching for the network or
 * needing live credentials.
 */
import { createHmac, timingSafeEqual } from 'node:crypto';

/**
 * Constant-time HMAC-SHA256 comparison.
 *
 * timingSafeEqual rather than `===` is the point: a normal string comparison
 * returns as soon as two bytes differ, and that timing difference leaks how much
 * of a guessed signature was correct, making forgery tractable byte by byte.
 *
 * Lengths are checked first because timingSafeEqual throws on mismatched
 * buffers; that check is safe to short-circuit, since a signature's length is
 * not secret.
 */
function hmacMatches(input: string, signature: string, secret: string): boolean {
  const expected = createHmac('sha256', secret).update(input).digest('hex');
  const expectedBuf = Buffer.from(expected, 'hex');
  const providedBuf = Buffer.from(signature, 'hex');
  if (expectedBuf.length !== providedBuf.length) return false;
  return timingSafeEqual(expectedBuf, providedBuf);
}

/**
 * Read API credentials from the environment, or null when unconfigured.
 *
 * Null rather than throwing, so billing endpoints can degrade deliberately in
 * environments where payments are not set up at all.
 */
export function getRazorpayCredentials(): { keyId: string; keySecret: string } | null {
  const keyId = process.env.RAZORPAY_KEY_ID;
  const keySecret = process.env.RAZORPAY_KEY_SECRET;
  if (!keyId || !keySecret) return null;
  return { keyId, keySecret };
}

export interface RazorpayOrder {
  id: string;
  amount: number;
  currency: string;
}

export interface RazorpayOrders {
  create(params: {
    amount: number;
    currency: string;
    notes?: Record<string, string>;
  }): Promise<RazorpayOrder>;
}

/** Live orders client. Required lazily so the SDK is not loaded when unused. */
export function realRazorpayOrders(keyId: string, keySecret: string): RazorpayOrders {
  return {
    async create(params) {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
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
  fetch(paymentId: string): Promise<RazorpayPaymentDetails>;
}

/** Live payments client, used to confirm what was actually charged. */
export function realRazorpayPayments(keySecret: string): RazorpayPayments {
  return {
    async fetch(paymentId) {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const Razorpay = require('razorpay');
      const client = new Razorpay({ key_id: process.env.RAZORPAY_KEY_ID, key_secret: keySecret });
      const payment = await client.payments.fetch(paymentId);
      return { orderId: payment.order_id, amount: payment.amount, notes: payment.notes };
    },
  };
}

export interface RazorpayRefunds {
  create(paymentId: string): Promise<{ id: string; status: string }>;
}

/** Live refunds client. An empty options object refunds the full amount. */
export function realRazorpayRefunds(keyId: string, keySecret: string): RazorpayRefunds {
  return {
    async create(paymentId) {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const Razorpay = require('razorpay');
      const client = new Razorpay({ key_id: keyId, key_secret: keySecret });
      const refund = await client.payments.refund(paymentId, {});
      return { id: refund.id, status: refund.status };
    },
  };
}

export interface RazorpaySubscriptionState {
  status: string;
}

export interface RazorpaySubscriptions {
  fetch(razorpaySubscriptionId: string): Promise<RazorpaySubscriptionState>;
}

/** Live subscriptions client, used by billing reconciliation. */
export function realRazorpaySubscriptions(keyId: string, keySecret: string): RazorpaySubscriptions {
  return {
    async fetch(razorpaySubscriptionId) {
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      const Razorpay = require('razorpay');
      const client = new Razorpay({ key_id: keyId, key_secret: keySecret });
      const sub = await client.subscriptions.fetch(razorpaySubscriptionId);
      return { status: sub.status };
    },
  };
}

/**
 * Verify the signature returned to the client after a payment.
 *
 * The signed string is orderId and paymentId joined by a pipe -- exactly the
 * format Razorpay specifies. Any deviation makes every legitimate signature
 * fail, so this must not be "tidied".
 */
export function verifyPaymentSignature(
  orderId: string,
  paymentId: string,
  signature: string,
  keySecret: string
): boolean {
  return hmacMatches(`${orderId}|${paymentId}`, signature, keySecret);
}

/**
 * Verify a webhook against the raw request body.
 *
 * Must be given the body exactly as received, before any JSON parse and
 * re-serialise: that round trip can reorder keys or alter whitespace, and the
 * HMAC is over the literal bytes Razorpay sent.
 */
export function verifyWebhookSignature(
  rawBody: string,
  signature: string,
  webhookSecret: string
): boolean {
  return hmacMatches(rawBody, signature, webhookSecret);
}
