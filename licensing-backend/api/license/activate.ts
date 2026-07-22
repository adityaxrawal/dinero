// Doc 30 TASK-LIC-002, Doc 19 §14.2: POST /api/license/activate
//
// Corrected during TASK-BILL-002 (real conflict found and resolved, see
// Doc 30 changelog): activation is a direct Razorpay payment confirmation,
// not a redeemable "license_key" -- matches the already-shipped, tested
// desktop client (src-tauri/src/licensing/client.rs::ActivateRequest)
// exactly: { email, razorpay_payment_id, razorpay_signature, device_id,
// billing_interval }, no license_key field anywhere.
//
// Default export is the thin Vercel Serverless entrypoint; `activateLicense`
// (named export) carries all real logic against the narrow ActivateDb
// interface and is unit-tested with zero HTTP/Vercel/live-DB dependency.
import type { Prisma, PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { signLicenseJwt } from '../../lib/jwt';
import { logAuditEvent, countRecentEvents, type AuditWriter } from '../../lib/audit';
import { verifyPaymentSignature, realRazorpayPayments, type RazorpayPayments } from '../../lib/razorpay';
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

const PLAN_BY_BILLING_INTERVAL: Record<string, { planId: string; billingInterval: string }> = {
  monthly: { planId: 'desktop_pro_monthly', billingInterval: 'monthly' },
  annual: { planId: 'desktop_pro_annual', billingInterval: 'annual' },
};

const RATE_LIMIT_WINDOW_MS = 60 * 60 * 1000; // 1 hour
const RATE_LIMIT_MAX_ATTEMPTS = 5;

export type ActivateDb = {
  account: Pick<PrismaClient['account'], 'findUnique' | 'create'>;
  licenseToken: Pick<PrismaClient['licenseToken'], 'findFirst' | 'upsert'>;
  subscription: Pick<PrismaClient['subscription'], 'create' | 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

export async function activateLicense(
  db: ActivateDb,
  input: ActivateInput,
  privateKeyPem: string,
  razorpayKeySecret: string,
  razorpayPayments: RazorpayPayments
): Promise<ActivateResult> {
  // Doc 30 TASK-LIC-002: "rate-limit activation attempts per key (e.g.
  // 5/hour)" -- rate-limited per email now, since there's no license key.
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

  // Doc 40 §4: "The backend independently verifies the payment signature
  // server-side... before ever trusting the client-supplied success claim."
  const payment = await razorpayPayments.fetch(input.razorpay_payment_id);
  const signatureValid = verifyPaymentSignature(payment.orderId, input.razorpay_payment_id, input.razorpay_signature, razorpayKeySecret);
  if (!signatureValid) {
    throw new LicensingApiError('PAYMENT_VERIFICATION_FAILED', 'Razorpay payment signature could not be verified');
  }

  const planInfo = PLAN_BY_BILLING_INTERVAL[input.billing_interval];
  if (!planInfo) {
    throw new LicensingApiError('VALIDATION_ERROR', 'Unknown billing_interval');
  }

  let account = await db.account.findUnique({ where: { email: input.email } });
  if (!account) {
    account = await db.account.create({ data: { email: input.email } });
  }

  // Doc 19 §14.2 backend enforcement: "looks up existing device bindings for
  // this license... if the device_id matches an existing binding
  // (re-activation on same Mac), refreshes and reissues the JWT; otherwise,
  // creates a new binding." accountId isn't unique on this table (a device
  // can be rebound over time, leaving historical rows), so this takes the
  // most recent one.
  const currentBinding = await db.licenseToken.findFirst({ where: { accountId: account.id }, orderBy: { createdAt: 'desc' } });

  if (currentBinding?.deviceFingerprint && currentBinding.deviceFingerprint !== input.device_id) {
    throw new LicensingApiError(
      'DEVICE_ALREADY_BOUND',
      `License already bound to another device (${maskDeviceFingerprint(currentBinding.deviceFingerprint)})`
    );
  }

  const now = new Date();
  const expiresAt = new Date(now.getTime() + 48 * 60 * 60 * 1000);

  const existingSubscription = await db.subscription.findFirst({ where: { accountId: account.id }, orderBy: { createdAt: 'desc' } });
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
    { sub: input.email, device_id: input.device_id, plan: planInfo.planId, billing_interval: planInfo.billingInterval },
    privateKeyPem
  );

  await logAuditEvent(db.licensingAuditLog, {
    accountId: account.id,
    eventType: 'license_activated',
    deviceFingerprint: input.device_id,
  });

  return { status: 'activated', jwt: jwtToken, plan: planInfo.planId, billing_interval: planInfo.billingInterval, expires_at: expiresAt.toISOString() };
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return;
  }
  const { email, razorpay_payment_id, razorpay_signature, device_id, billing_interval } = req.body ?? {};
  if (!email || !razorpay_payment_id || !razorpay_signature || !device_id || !billing_interval) {
    res.status(400).json({ code: 'VALIDATION_ERROR', message: 'email, razorpay_payment_id, razorpay_signature, device_id, and billing_interval are required' });
    return;
  }
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
    if (e instanceof LicensingApiError) {
      res.status(e.code === 'RATE_LIMITED' ? 429 : 400).json({ code: e.code, message: e.message, details: e.details });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}
