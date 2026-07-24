import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-LIC-003, Doc 19 (license_validate): POST /api/license/validate
// Called on cold-start/resume per the hybrid JWT model (Doc 15/22): a network
// call here, offline signature verification for everything else.
//
// Corrected during TASK-BILL-002 (real conflict found and resolved, see
// Doc 30 changelog): matches the already-shipped desktop client exactly --
// `ValidateRequest { device_id }` only, no license_key, no email. device_id
// alone is the lookup key (one device is bound to at most one account's
// license at a time).
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
  licenseToken: {
    findUnique(args: {
      where: { deviceFingerprint: string };
      include: { account: true };
    }): Promise<{ accountId: string; account: { email: string } } | null>;
  };
  subscription: Pick<PrismaClient['subscription'], 'findFirst'>;
  licensingAuditLog: AuditWriter;
};

export async function validateLicense(
  db: ValidateDb,
  input: ValidateInput,
  privateKeyPem: string
): Promise<ValidateResult> {
  const { token, subscription } = await getTokenAndSubscription(db, input.device_id, true);
  if (!subscription) {
    throw new LicensingApiError('LICENSE_INVALID', 'No subscription found for this license');
  }

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
