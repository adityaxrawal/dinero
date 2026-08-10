/**
 * Shared request-handling helpers for the licensing endpoints.
 *
 * Each endpoint repeats the same opening moves -- reject non-POST, check the
 * required fields are present, resolve the device to its token and current
 * subscription -- so those live here rather than being restated per route.
 */
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { LicensingApiError, sendApiError } from './errors';

/**
 * Gate a request on method and required body fields.
 *
 * Writes the error response itself and returns false, so callers can guard with
 * a single early return rather than threading a validation result. Note the
 * check is truthiness-based, so an empty string or a zero is treated as absent.
 */
export function requirePostWithFields(
  req: VercelRequest,
  res: VercelResponse,
  requiredFields: string[]
): boolean {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return false;
  }
  
  for (const field of requiredFields) {
    if (!req.body || !req.body[field]) {
      res.status(400).json({ code: 'VALIDATION_ERROR', message: `${field} is required` });
      return false;
    }
  }

  return true;
}

interface FindDelegate {
  findFirst(args: Record<string, unknown>): Promise<{ accountId: string } | null>;
  findUnique?(args: Record<string, unknown>): Promise<{ accountId: string } | null>;
}

/**
 * Resolve a device fingerprint to its license token and latest subscription.
 *
 * A device with no token is an immediate LICENSE_INVALID -- the caller is
 * claiming an entitlement that was never issued.
 *
 * The subscription is fetched separately and ordered newest-first because an
 * account accumulates rows over time through renewals, cancellations and
 * re-subscriptions; only the most recent one describes current entitlement. It
 * may legitimately be null, which is the state during a trial.
 */
export async function getTokenAndSubscription(
  db: {
    licenseToken: FindDelegate;
    subscription: Pick<FindDelegate, 'findFirst'>;
  },
  deviceId: string,
  includeAccount: boolean = false
) {
  const method = includeAccount && db.licenseToken.findUnique ? db.licenseToken.findUnique : db.licenseToken.findFirst;
  const token = await method({
    where: { deviceFingerprint: deviceId },
    ...(includeAccount ? { include: { account: true } } : {}),
  });
  
  if (!token) {
    throw new LicensingApiError('LICENSE_INVALID', 'No license bound to this device');
  }

  const subscription = await db.subscription.findFirst({
    where: { accountId: token.accountId },
    orderBy: { createdAt: 'desc' },
  });

  return { token, subscription };
}

/**
 * Error responder for the admin support routes.
 *
 * Maps a missing record to 404 and everything else to 400, so support tooling
 * can distinguish "no such account" from "the request was malformed".
 */
export function handleAdminSupportError(res: VercelResponse, e: unknown) {
  sendApiError(res, e, { statusFor: (code) => (code === 'NOT_FOUND' ? 404 : 400) });
}
