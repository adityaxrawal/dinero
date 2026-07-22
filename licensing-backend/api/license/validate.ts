// Doc 30 TASK-LIC-003, Doc 19 (license_validate): POST /api/license/validate
// Called on cold-start/resume per the hybrid JWT model (Doc 15/22): a network
// call here, offline signature verification for everything else.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { hashLicenseKey } from '../../lib/license_key';
import { signLicenseJwt } from '../../lib/jwt';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface ValidateInput {
  license_key: string;
  device_id: string;
}

/// Doc 30 TASK-LIC-003: "GRACE is computed client-side, not server-side, per
/// TASK-AUTH-009" -- this endpoint only ever reports what it actually knows
/// server-side. LOCKED here means "backend considers the subscription dead"
/// (canceled/expired), not the 7-day-offline-grace-elapsed LOCKED the
/// desktop computes independently.
export type ServerLicenseState = 'TRIAL' | 'ACTIVE' | 'PAST_DUE' | 'LOCKED';

export interface ValidateResult {
  status: 'validated';
  state: ServerLicenseState;
  jwt: string;
  plan: string;
  billing_interval: string;
  expires_at: string;
  server_time: string;
}

function computeState(subscriptionStatus: string): ServerLicenseState {
  switch (subscriptionStatus) {
    case 'trialing':
      return 'TRIAL';
    case 'active':
      return 'ACTIVE';
    case 'past_due':
      return 'PAST_DUE';
    default:
      // canceled | expired | anything unrecognized
      return 'LOCKED';
  }
}

export type ValidateDb = {
  licenseToken: Pick<PrismaClient['licenseToken'], 'findUnique'>;
  subscription: Pick<PrismaClient['subscription'], 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

export async function validateLicense(db: ValidateDb, input: ValidateInput, privateKeyPem: string, accountEmail: string): Promise<ValidateResult> {
  const licenseKeyHash = hashLicenseKey(input.license_key);
  const token = await db.licenseToken.findUnique({ where: { licenseKeyHash } });
  if (!token) {
    throw new LicensingApiError('LICENSE_INVALID', 'Unknown license key');
  }

  // Doc 30 TASK-LIC-003: "a mismatch should be rare, indicating tampering,
  // since the desktop app should only ever validate for a device it
  // activated itself" -- DEVICE_MISMATCH, not DEVICE_ALREADY_BOUND (that's
  // activate's rejection of a *new* device; this is "this isn't even the
  // device this token was issued to").
  if (token.deviceFingerprint !== input.device_id) {
    throw new LicensingApiError('DEVICE_MISMATCH', 'Device fingerprint does not match the bound device');
  }

  const subscription = await db.subscription.findFirst({
    where: { accountId: token.accountId },
    orderBy: { createdAt: 'desc' },
  });
  if (!subscription) {
    throw new LicensingApiError('LICENSE_INVALID', 'No subscription found for this license');
  }

  const state = computeState(subscription.status);
  const now = new Date();
  const expiresAt = new Date(now.getTime() + 48 * 60 * 60 * 1000);

  const jwtToken = signLicenseJwt(
    { sub: accountEmail, device_id: input.device_id, plan: subscription.planId, billing_interval: subscription.billingInterval },
    privateKeyPem
  );

  await logAuditEvent(db.licensingAuditLog, {
    accountId: token.accountId,
    eventType: 'license_validated',
    deviceFingerprint: input.device_id,
    payload: { state },
  });

  return {
    status: 'validated',
    state,
    jwt: jwtToken,
    plan: subscription.planId,
    billing_interval: subscription.billingInterval,
    expires_at: expiresAt.toISOString(),
    server_time: now.toISOString(),
  };
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return;
  }
  const { license_key, device_id } = req.body ?? {};
  if (!license_key || !device_id) {
    res.status(400).json({ code: 'VALIDATION_ERROR', message: 'license_key and device_id are required' });
    return;
  }
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  if (!privateKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const licenseKeyHash = hashLicenseKey(license_key);
    const token = await prisma.licenseToken.findUnique({ where: { licenseKeyHash }, include: { account: true } });
    const email = token?.account?.email ?? '';
    const result = await validateLicense(prisma, { license_key, device_id }, privateKeyPem, email);
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}
