import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-LIC-008: POST /api/license/refresh-token
// Obtains a freshly-signed JWT without full re-activation, used as a
// currently-valid JWT approaches expiry to minimize user-visible friction.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields, getTokenAndSubscription } from '../../lib/api_helpers';
import { signLicenseJwt, verifyLicenseJwt, JwtVerificationError } from '../../lib/jwt';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface RefreshInput {
  jwt: string;
  device_id: string;
}

export interface RefreshResult {
  status: 'refreshed';
  jwt: string;
  expires_at: string;
}

/// Doc 30 TASK-LIC-008: "Rejects refresh attempts for JWTs already past a
/// hard maximum staleness window (e.g. expired >48 hours ago), forcing a
/// full validate call instead so long-offline scenarios always get an
/// authoritative status check rather than perpetual refresh chaining."
const MAX_STALENESS_MS = 48 * 60 * 60 * 1000;

export type RefreshDb = {
  licenseToken: Pick<PrismaClient['licenseToken'], 'findFirst'>;
  subscription: Pick<PrismaClient['subscription'], 'findFirst'>;
  licensingAuditLog?: AuditWriter;
};

export async function refreshLicenseToken(
  db: RefreshDb,
  input: RefreshInput,
  publicKeyPem: string,
  privateKeyPem: string
): Promise<RefreshResult> {
  // Signature must still be valid; expiration is deliberately NOT enforced
  // here -- a token approaching or just past its exp is exactly the normal
  // case this endpoint exists for. Staleness is checked explicitly below.
  let claims;
  try {
    claims = verifyLicenseJwt(input.jwt, publicKeyPem, { ignoreExpiration: true });
  } catch (e) {
    // Doc 30 TASK-LIC-009: a failed signature verification is one of the
    // three backend-observable fraud signals -- logged here (the only place
    // an incoming JWT's signature is checked) so fraud_detection.ts's scan
    // has a real producer, not just a theoretical event type.
    if (e instanceof JwtVerificationError && db.licensingAuditLog) {
      await logAuditEvent(db.licensingAuditLog, {
        eventType: 'jwt_verification_failed',
        deviceFingerprint: input.device_id,
      });
    }
    throw e;
  }

  if (claims.device_id !== input.device_id) {
    throw new LicensingApiError(
      'DEVICE_MISMATCH',
      'JWT device_id does not match the requesting device'
    );
  }

  const nowMs = Date.now();
  const staleness = nowMs - claims.exp * 1000;
  if (staleness > MAX_STALENESS_MS) {
    throw new LicensingApiError(
      'LICENSE_INVALID',
      'Token too stale to refresh -- call validate instead'
    );
  }

  // Called for its validation side effect too: it throws LICENSE_INVALID when
  // no token is bound to this device. Only the subscription is read here.
  const { subscription } = await getTokenAndSubscription(db, input.device_id);
  if (!subscription || !['trialing', 'active', 'past_due'].includes(subscription.status)) {
    throw new LicensingApiError('LICENSE_INVALID', 'Subscription is no longer refreshable');
  }

  const now = new Date();
  const expiresAt = new Date(now.getTime() + 48 * 60 * 60 * 1000);
  const newJwt = signLicenseJwt(
    {
      sub: claims.sub,
      device_id: input.device_id,
      plan: subscription.planId,
      billing_interval: subscription.billingInterval,
    },
    privateKeyPem
  );

  return { status: 'refreshed', jwt: newJwt, expires_at: expiresAt.toISOString() };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  if (!requirePostWithFields(req, res, ['jwt', 'device_id'])) return;
  const { jwt: currentJwt, device_id } = req.body;
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  const publicKeyPem = process.env.JWT_PUBLIC_KEY_PEM;
  if (!privateKeyPem || !publicKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await refreshLicenseToken(
      prisma,
      { jwt: currentJwt, device_id },
      publicKeyPem,
      privateKeyPem
    );
    res.status(200).json(result);
  } catch (e) {
    sendApiError(res, e);
  }
}

export default withRequestLogging('license/refresh-token', handler);
