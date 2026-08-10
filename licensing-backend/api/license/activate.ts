/**
 * License activation: exchanges a verified Razorpay payment for a signed token.
 *
 * The most security-sensitive endpoint in the service, since a flaw here grants
 * a free license. It proceeds through four gates in a deliberate order:
 *
 *   1. Rate limiting, counted per email over the audit log, which bounds
 *      brute-force attempts against signature verification.
 *   2. Payment verification. The payment is fetched from Razorpay and its
 *      signature checked -- crucially, the order id comes from Razorpay's own
 *      record rather than from the request, so a caller cannot supply a
 *      matching id/signature pair of their own construction.
 *   3. Device binding. A license already bound elsewhere is refused.
 *   4. Issue and record. The subscription and token are written and a signed
 *      JWT is returned.
 *
 * The core logic is exported separately from the HTTP handler and takes its
 * database and Razorpay client as parameters, so the whole flow is testable
 * without a live payment provider.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { Prisma, PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma, findOrCreateAccount } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields } from '../../lib/api_helpers';
import { signLicenseJwt } from '../../lib/jwt';
import { logAuditEvent, countRecentEvents, type AuditWriter } from '../../lib/audit';
import {
  verifyPaymentSignature,
  realRazorpayPayments,
  type RazorpayPayments,
} from '../../lib/razorpay';
import { maskDeviceFingerprint } from '../../lib/license_key';

export interface ActivateInput {
  email: string;
  razorpay_payment_id: string;
  razorpay_signature: string;
  device_id: string;
  billing_interval: 'monthly' | 'annual';
}

export interface ActivateResult {
  status: 'activated';
  jwt: string;
  plan: string;
  billing_interval: string;
  expires_at: string;
}

// Billing interval to plan. An unrecognised interval is rejected rather than
// defaulted, so a malformed request can never silently buy the cheaper plan.
const PLAN_BY_BILLING_INTERVAL: Record<string, { planId: string; billingInterval: string }> = {
  monthly: { planId: 'desktop_pro_monthly', billingInterval: 'monthly' },
  annual: { planId: 'desktop_pro_annual', billingInterval: 'annual' },
};

// Five attempts per hour per email. Generous for genuine retries, restrictive
// enough to make signature guessing impractical.
const RATE_LIMIT_WINDOW_MS = 60 * 60 * 1000;
const RATE_LIMIT_MAX_ATTEMPTS = 5;

export type ActivateDb = {
  account: Pick<PrismaClient['account'], 'findUnique' | 'create'>;
  licenseToken: Pick<PrismaClient['licenseToken'], 'findFirst' | 'upsert'>;
  subscription: Pick<PrismaClient['subscription'], 'create' | 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

/**
 * Run the activation flow and return the issued token.
 *
 * Throws LicensingApiError with a specific code at each gate, which the handler
 * maps to a status. Every outcome is written to the audit log.
 */
export async function activateLicense(
  db: ActivateDb,
  input: ActivateInput,
  privateKeyPem: string,
  razorpayKeySecret: string,
  razorpayPayments: RazorpayPayments
): Promise<ActivateResult> {
  // The attempt is recorded before anything is validated, so that failed and
  // abandoned attempts are counted too -- the rate limit below reads this log,
  // and only logging successes would make it trivially bypassable.
  await logAuditEvent(db.licensingAuditLog, {
    eventType: 'activation_attempt',
    payload: { email: input.email, device_id: input.device_id } as Prisma.InputJsonValue,
  });
  const recentAttempts = await countRecentEvents(
    db.licensingAuditLog,
    'activation_attempt',
    RATE_LIMIT_WINDOW_MS,
    (payload) => (payload as { email?: string } | null)?.email === input.email
  );
  if (recentAttempts > RATE_LIMIT_MAX_ATTEMPTS) {
    throw new LicensingApiError('RATE_LIMITED', 'Too many activation attempts for this account');
  }

  // The order id is taken from Razorpay's record of the payment, never from the
  // request body. This is what stops a caller pairing an arbitrary order id with
  // a signature they generated themselves.
  const payment = await razorpayPayments.fetch(input.razorpay_payment_id);
  const signatureValid = verifyPaymentSignature(
    payment.orderId,
    input.razorpay_payment_id,
    input.razorpay_signature,
    razorpayKeySecret
  );
  if (!signatureValid) {
    throw new LicensingApiError(
      'PAYMENT_VERIFICATION_FAILED',
      'Razorpay payment signature could not be verified'
    );
  }

  const planInfo = PLAN_BY_BILLING_INTERVAL[input.billing_interval];
  if (!planInfo) {
    throw new LicensingApiError('VALIDATION_ERROR', 'Unknown billing_interval');
  }

  const account = await findOrCreateAccount(db, input.email);

  const currentBinding = await db.licenseToken.findFirst({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
  });

  // One device per license. Re-activating on the same device is permitted and
  // idempotent; a different device is refused. The existing fingerprint is
  // masked in the message so the response cannot enumerate a user's machines.
  if (currentBinding?.deviceFingerprint && currentBinding.deviceFingerprint !== input.device_id) {
    throw new LicensingApiError(
      'DEVICE_ALREADY_BOUND',
      `License already bound to another device (${maskDeviceFingerprint(currentBinding.deviceFingerprint)})`
    );
  }

  // Token lifetime matches the JWT default: short enough to bound a revoked
  // subscription, long enough to survive a spell offline.
  const now = new Date();
  const expiresAt = new Date(now.getTime() + 48 * 60 * 60 * 1000);

  const existingSubscription = await db.subscription.findFirst({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
  });
  // Only create a subscription if none exists. A returning customer re-
  // activating already has one, and a second row would corrupt the billing
  // history and the metrics derived from it.
  if (!existingSubscription) {
    await db.subscription.create({
      data: {
        accountId: account.id,
        planId: planInfo.planId,
        status: 'active',
        billingInterval: planInfo.billingInterval,
        currentPeriodStart: now,
      },
    });
  }

  // Upsert keyed on the fingerprint, so re-activation refreshes the existing
  // token in place. Clearing revokedAt is what makes this the reactivation path
  // for a previously deactivated device.
  await db.licenseToken.upsert({
    where: { deviceFingerprint: input.device_id },
    update: { jwtIssuedAt: now, jwtExpiresAt: expiresAt, revokedAt: null },
    create: {
      accountId: account.id,
      deviceFingerprint: input.device_id,
      deviceBoundAt: now,
      jwtIssuedAt: now,
      jwtExpiresAt: expiresAt,
    },
  });

  const jwtToken = signLicenseJwt(
    {
      sub: input.email,
      device_id: input.device_id,
      plan: planInfo.planId,
      billing_interval: planInfo.billingInterval,
    },
    privateKeyPem
  );

  await logAuditEvent(db.licensingAuditLog, {
    accountId: account.id,
    eventType: 'license_activated',
    deviceFingerprint: input.device_id,
  });

  return {
    status: 'activated',
    jwt: jwtToken,
    plan: planInfo.planId,
    billing_interval: planInfo.billingInterval,
    expires_at: expiresAt.toISOString(),
  };
}

/**
 * HTTP wrapper: validate the request, load secrets, delegate, map errors.
 *
 * Missing server configuration returns 500 rather than a validation error --
 * the request was fine; the deployment is not.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  const required = ['email', 'razorpay_payment_id', 'razorpay_signature', 'device_id', 'billing_interval'];
  if (!requirePostWithFields(req, res, required)) return;
  const { email, razorpay_payment_id, razorpay_signature, device_id, billing_interval } = req.body;
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  const razorpayKeySecret = process.env.RAZORPAY_KEY_SECRET;
  if (!privateKeyPem || !razorpayKeySecret) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await activateLicense(
      prisma,
      { email, razorpay_payment_id, razorpay_signature, device_id, billing_interval },
      privateKeyPem,
      razorpayKeySecret,
      realRazorpayPayments(razorpayKeySecret)
    );
    res.status(200).json(result);
  } catch (e) {
    sendApiError(res, e, {
      statusFor: (code) => (code === 'RATE_LIMITED' ? 429 : 400),
      includeDetails: true,
    });
  }
}

// Wrapped so every activation attempt emits a correlated, timed log line.
export default withRequestLogging('license/activate', handler);
