/**
 * License validation: the desktop app's periodic entitlement check.
 *
 * Called on launch and on a schedule. Given a device fingerprint it returns the
 * current entitlement state along with a freshly signed token, so a client that
 * can reach the server always leaves with a valid one.
 *
 * The subscription status is the source of truth here and is mapped to the
 * state the client understands; the client never decides its own entitlement.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields, getTokenAndSubscription } from '../../lib/api_helpers';
import { signLicenseJwt } from '../../lib/jwt';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface ValidateInput {
  device_id: string;
}

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

/**
 * Maps a subscription status onto the entitlement state the client understands.
 *
 * The default is LOCKED, which is the safe direction: an unrecognised status must
 * never be interpreted as granting access.
 */
function computeState(subscriptionStatus: string): ServerLicenseState {
  switch (subscriptionStatus) {
    case 'trialing':
      return 'TRIAL';
    case 'active':
      return 'ACTIVE';
    case 'past_due':
      return 'PAST_DUE';
    default:
      return 'LOCKED';
  }
}

export type ValidateDb = {
  licenseToken: {
    findUnique(args: {
      where: { deviceFingerprint: string };
      include: { account: true };
    }): Promise<{ accountId: string; account: { email: string } } | null>;
  };
  subscription: Pick<PrismaClient['subscription'], 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

/**
 * Resolve a device to its current entitlement and mint a fresh token.
 *
 * An unknown device or an account with no subscription is LICENSE_INVALID --
 * both mean there is nothing to validate.
 */
export async function validateLicense(
  db: ValidateDb,
  input: ValidateInput,
  privateKeyPem: string
): Promise<ValidateResult> {
  const { token, subscription } = await getTokenAndSubscription(db, input.device_id, true);
  if (!subscription) {
    throw new LicensingApiError('LICENSE_INVALID', 'No subscription found for this license');
  }

  // Server-side status is authoritative. Deriving the state here rather than
  // shipping the raw status keeps entitlement policy on the server, where a
  // modified client cannot reinterpret it.
  const state = computeState(subscription.status);
  const now = new Date();
  const expiresAt = new Date(now.getTime() + 48 * 60 * 60 * 1000);

  const jwtToken = signLicenseJwt(
    {
      sub: token.account.email,
      device_id: input.device_id,
      plan: subscription.planId,
      billing_interval: subscription.billingInterval,
    },
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

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  if (!requirePostWithFields(req, res, ['device_id'])) return;
  const { device_id } = req.body;
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  if (!privateKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await validateLicense(prisma, { device_id }, privateKeyPem);
    res.status(200).json(result);
  } catch (e) {
    sendApiError(res, e);
  }
}

export default withRequestLogging('license/validate', handler);
