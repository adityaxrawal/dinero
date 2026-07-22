// Doc 30 TASK-LIC-002, Doc 19 §14.2: POST /api/license/activate
//
// Default export is the thin Vercel Serverless entrypoint; `activateLicense`
// (named export) carries all real logic against the narrow ActivateDb
// interface and is unit-tested with zero HTTP/Vercel/live-DB dependency.
import type { Prisma, PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { hashLicenseKey, maskDeviceFingerprint } from '../../lib/license_key';
import { signLicenseJwt } from '../../lib/jwt';
import { logAuditEvent, countRecentEvents, type AuditWriter } from '../../lib/audit';

export interface ActivateInput {
  license_key: string;
  device_id: string;
  email: string;
}

export interface ActivateResult {
  status: 'activated';
  jwt: string;
  plan: string;
  billing_interval: string;
  expires_at: string;
}

/// Statuses treated as "activatable" -- everything else (past_due, grace,
/// locked, canceled, expired) rejects with LICENSE_INVALID. GRACE/LOCKED are
/// computed client-side (Doc 30 TASK-LIC-003), so they never appear here as
/// a server-persisted subscription.status value in the first place.
const ACTIVATABLE_STATUSES = new Set(['trialing', 'active']);

const RATE_LIMIT_WINDOW_MS = 60 * 60 * 1000; // 1 hour
const RATE_LIMIT_MAX_ATTEMPTS = 5;

export type ActivateDb = {
  licenseToken: Pick<PrismaClient['licenseToken'], 'findUnique' | 'update'>;
  subscription: Pick<PrismaClient['subscription'], 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

export async function activateLicense(db: ActivateDb, input: ActivateInput, privateKeyPem: string): Promise<ActivateResult> {
  const licenseKeyHash = hashLicenseKey(input.license_key);

  // Doc 30 TASK-LIC-002: log the attempt before the rate-limit check itself
  // runs, so a brute-force pattern across many keys is still visible.
  await logAuditEvent(db.licensingAuditLog, {
    eventType: 'activation_attempt',
    payload: { license_key_hash: licenseKeyHash, device_id: input.device_id } as Prisma.InputJsonValue,
  });

  const recentAttempts = await countRecentEvents(
    db.licensingAuditLog,
    'activation_attempt',
    RATE_LIMIT_WINDOW_MS,
    (payload) => (payload as { license_key_hash?: string } | null)?.license_key_hash === licenseKeyHash
  );
  if (recentAttempts > RATE_LIMIT_MAX_ATTEMPTS) {
    throw new LicensingApiError('RATE_LIMITED', 'Too many activation attempts for this license key');
  }

  const token = await db.licenseToken.findUnique({ where: { licenseKeyHash } });
  if (!token) {
    throw new LicensingApiError('LICENSE_INVALID', 'Unknown license key');
  }

  // Doc 30 TASK-LIC-002: device binding -- a license bound to a *different*
  // hardware UUID is rejected, masked identifier only, never the full UUID.
  if (token.deviceFingerprint && token.deviceFingerprint !== input.device_id) {
    throw new LicensingApiError(
      'DEVICE_ALREADY_BOUND',
      `License already bound to another device (${maskDeviceFingerprint(token.deviceFingerprint)})`
    );
  }

  const subscription = await db.subscription.findFirst({
    where: { accountId: token.accountId },
    orderBy: { createdAt: 'desc' },
  });
  if (!subscription || !ACTIVATABLE_STATUSES.has(subscription.status)) {
    throw new LicensingApiError('LICENSE_INVALID', 'License is not active or trialing');
  }

  const now = new Date();
  const expiresAt = new Date(now.getTime() + 48 * 60 * 60 * 1000);
  const jwtToken = signLicenseJwt(
    {
      sub: input.email,
      device_id: input.device_id,
      plan: subscription.planId,
      billing_interval: subscription.billingInterval,
    },
    privateKeyPem
  );

  // First activation (device_fingerprint was null) binds the device now;
  // re-activation from the same already-bound device is a no-op update.
  if (!token.deviceFingerprint) {
    await db.licenseToken.update({
      where: { id: token.id },
      data: { deviceFingerprint: input.device_id, deviceBoundAt: now, jwtIssuedAt: now, jwtExpiresAt: expiresAt },
    });
  } else {
    await db.licenseToken.update({
      where: { id: token.id },
      data: { jwtIssuedAt: now, jwtExpiresAt: expiresAt },
    });
  }

  await logAuditEvent(db.licensingAuditLog, {
    accountId: token.accountId,
    eventType: 'license_activated',
    deviceFingerprint: input.device_id,
  });

  return {
    status: 'activated',
    jwt: jwtToken,
    plan: subscription.planId,
    billing_interval: subscription.billingInterval,
    expires_at: expiresAt.toISOString(),
  };
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return;
  }
  const { license_key, device_id, email } = req.body ?? {};
  if (!license_key || !device_id || !email) {
    res.status(400).json({ code: 'VALIDATION_ERROR', message: 'license_key, device_id, and email are required' });
    return;
  }
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  if (!privateKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }

  try {
    const result = await activateLicense(prisma, { license_key, device_id, email }, privateKeyPem);
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(e.code === 'RATE_LIMITED' ? 429 : 400).json({ code: e.code, message: e.message, details: e.details });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}
