/**
 * Token refresh: renews a still-legitimate license without re-validating fully.
 *
 * The lighter counterpart to validate, for a client whose token has expired or
 * is close to it. Three checks gate the renewal:
 *
 *   - The old token's signature must verify, though expiry is ignored -- an
 *     expired token is precisely what is being refreshed.
 *   - Its device_id must match the caller, so a token copied to another machine
 *     cannot be renewed there.
 *   - It must not be too stale. Beyond that window the client is sent to
 *     validate instead, which re-checks the subscription from scratch.
 *
 * A failed signature check is audited, since it is one of the fraud signals.
 */
import { withRequestLogging } from '../../lib/request_logging';
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

const MAX_STALENESS_MS = 48 * 60 * 60 * 1000;

export type RefreshDb = {
  licenseToken: Pick<PrismaClient['licenseToken'], 'findFirst'>;
  subscription: Pick<PrismaClient['subscription'], 'findFirst'>;
  licensingAuditLog?: AuditWriter;
};

/**
 * Verify an existing token and issue its replacement.
 */
export async function refreshLicenseToken(
  db: RefreshDb,
  input: RefreshInput,
  publicKeyPem: string,
  privateKeyPem: string
): Promise<RefreshResult> {
  let claims;
  try {
    claims = verifyLicenseJwt(input.jwt, publicKeyPem, { ignoreExpiration: true });
  } catch (e) {
    if (e instanceof JwtVerificationError && db.licensingAuditLog) {
      await logAuditEvent(db.licensingAuditLog, {
        eventType: 'jwt_verification_failed',
        deviceFingerprint: input.device_id,
      });
    }
    throw e;
  }

  // Binding check: the token must belong to the device presenting it. Without
  // this, a leaked token could be refreshed indefinitely from anywhere.
  if (claims.device_id !== input.device_id) {
    throw new LicensingApiError(
      'DEVICE_MISMATCH',
      'JWT device_id does not match the requesting device'
    );
  }

  // Staleness is measured from expiry, not issuance. A long-dormant install
  // must go through full validation rather than quietly renewing a token that
  // predates a cancellation.
  const nowMs = Date.now();
  const staleness = nowMs - claims.exp * 1000;
  if (staleness > MAX_STALENESS_MS) {
    throw new LicensingApiError(
      'LICENSE_INVALID',
      'Token too stale to refresh -- call validate instead'
    );
  }

  // Refreshable states only. past_due is included deliberately -- that is the
  // grace period, where access continues while payment is retried.
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

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
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
