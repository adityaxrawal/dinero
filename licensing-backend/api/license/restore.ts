/**
 * Licence restore: re-binds a paying account's licence to this device after a
 * reinstall lost the local token (review M16 follow-up).
 *
 * validate and deactivate now require the device's current JWT, so an install
 * without one cannot recover through them. Restore asks instead for what the
 * customer has from their purchase: the account email and the Razorpay payment
 * id from the receipt. The payment must already be recorded against that
 * account (activation records it), so a payment id alone, or an email alone,
 * is not enough. A licence bound to a different device moves here and the old
 * binding is revoked (a lost or replaced Mac cannot deactivate itself), at most
 * once per 30 days per account; past that, support resets the binding.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { Prisma, PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields } from '../../lib/api_helpers';
import {
  signLicenseJwt,
  stateForSubscriptionStatus,
  LICENSE_TOKEN_TTL_MS,
  type ServerLicenseState,
} from '../../lib/jwt';
import { logAuditEvent, countRecentEvents, type AuditWriter } from '../../lib/audit';
import { maskDeviceFingerprint } from '../../lib/license_key';

export interface RestoreInput {
  email: string;
  razorpay_payment_id: string;
  device_id: string;
}

export interface RestoreResult {
  status: 'restored';
  state: ServerLicenseState;
  jwt: string;
  expires_at: string;
}

const RATE_LIMIT_WINDOW_MS = 60 * 60 * 1000;
const RATE_LIMIT_MAX_ATTEMPTS = 5;
const REBIND_WINDOW_MS = 30 * 24 * 60 * 60 * 1000;

export type RestoreDb = {
  account: Pick<PrismaClient['account'], 'findUnique'>;
  paymentProviderRecord: Pick<PrismaClient['paymentProviderRecord'], 'findFirst'>;
  licenseToken: Pick<PrismaClient['licenseToken'], 'findFirst' | 'upsert' | 'updateMany'>;
  subscription: Pick<PrismaClient['subscription'], 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

/** Restores the licence to this device and issues a fresh token. */
export async function restoreLicense(
  db: RestoreDb,
  input: RestoreInput,
  privateKeyPem: string
): Promise<RestoreResult> {
  await logAuditEvent(db.licensingAuditLog, {
    eventType: 'restore_attempt',
    payload: { email: input.email, device_id: input.device_id } as Prisma.InputJsonValue,
  });
  const recent = await countRecentEvents(
    db.licensingAuditLog,
    'restore_attempt',
    RATE_LIMIT_WINDOW_MS,
    (p) => (p as { email?: string } | null)?.email === input.email
  );
  if (recent > RATE_LIMIT_MAX_ATTEMPTS) {
    throw new LicensingApiError('RATE_LIMITED', 'Too many restore attempts for this account');
  }

  // One generic message for every mismatch, so the endpoint cannot be used to
  // learn which emails or payment ids exist.
  const notFound = () =>
    new LicensingApiError('LICENSE_INVALID', 'No licence matches that email and payment id');
  const account = await db.account.findUnique({ where: { email: input.email } });
  if (!account) throw notFound();
  const payment = await db.paymentProviderRecord.findFirst({
    where: { razorpayPaymentId: input.razorpay_payment_id, accountId: account.id },
  });
  if (!payment) throw notFound();

  const binding = await db.licenseToken.findFirst({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
  });
  const oldDevice =
    binding?.deviceFingerprint && binding.deviceFingerprint !== input.device_id
      ? binding.deviceFingerprint
      : null;
  if (oldDevice) {
    const rebinds = await countRecentEvents(
      db.licensingAuditLog,
      'license_rebind',
      REBIND_WINDOW_MS,
      (p) => (p as { account_id?: string } | null)?.account_id === account.id
    );
    if (rebinds > 0) {
      throw new LicensingApiError(
        'DEVICE_ALREADY_BOUND',
        `License already moved to another device (${maskDeviceFingerprint(oldDevice)}) in the last 30 days. Contact support to move it again.`
      );
    }
  }
  const subscription = await db.subscription.findFirst({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
  });
  if (!subscription) throw notFound();

  const now = new Date();
  if (oldDevice) {
    // Same release as admin/support_reset_binding, for every other device row.
    await db.licenseToken.updateMany({
      where: { accountId: account.id, NOT: { deviceFingerprint: input.device_id } },
      data: { deviceFingerprint: null, deviceBoundAt: null, revokedAt: now },
    });
    await logAuditEvent(db.licensingAuditLog, {
      accountId: account.id,
      eventType: 'license_rebind',
      deviceFingerprint: input.device_id,
      payload: { account_id: account.id, from_device: oldDevice } as Prisma.InputJsonValue,
    });
  }
  const expiresAt = new Date(now.getTime() + LICENSE_TOKEN_TTL_MS);
  await db.licenseToken.upsert({
    where: { deviceFingerprint: input.device_id },
    update: { accountId: account.id, jwtIssuedAt: now, jwtExpiresAt: expiresAt, revokedAt: null },
    create: {
      accountId: account.id,
      deviceFingerprint: input.device_id,
      deviceBoundAt: now,
      jwtIssuedAt: now,
      jwtExpiresAt: expiresAt,
    },
  });

  const state = stateForSubscriptionStatus(subscription.status, subscription.currentPeriodEnd);
  const jwt = signLicenseJwt(
    {
      sub: account.id,
      device_id: input.device_id,
      plan: subscription.planId,
      billing_interval: subscription.billingInterval,
      state,
    },
    privateKeyPem
  );
  await logAuditEvent(db.licensingAuditLog, {
    accountId: account.id,
    eventType: 'license_restored',
    deviceFingerprint: input.device_id,
  });
  return { status: 'restored', state, jwt, expires_at: expiresAt.toISOString() };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  if (!requirePostWithFields(req, res, ['email', 'razorpay_payment_id', 'device_id'])) return;
  const { email, razorpay_payment_id, device_id } = req.body;
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  if (!privateKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await restoreLicense(prisma, { email, razorpay_payment_id, device_id }, privateKeyPem);
    res.status(200).json(result);
  } catch (e) {
    sendApiError(res, e, { statusFor: (code) => (code === 'RATE_LIMITED' ? 429 : 400) });
  }
}

export default withRequestLogging('license/restore', handler);
