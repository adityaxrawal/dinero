import type { VercelRequest, VercelResponse } from '@vercel/node';
import { LicensingApiError, sendApiError } from './errors';

/**
 * Validates that the request is a POST and contains all required body fields.
 * Returns true if valid, false if an error response was sent.
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

/**
 * The two Prisma delegate methods this helper calls, structurally typed so it
 * accepts both the real client and the test doubles. `args`/result stay open —
 * Prisma's own generated argument and payload types vary per call shape, and
 * only `accountId` is read off the result here.
 */
interface FindDelegate {
  findFirst(args: Record<string, unknown>): Promise<{ accountId: string } | null>;
  findUnique?(args: Record<string, unknown>): Promise<{ accountId: string } | null>;
}

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

export function handleAdminSupportError(res: VercelResponse, e: unknown) {
  sendApiError(res, e, { statusFor: (code) => (code === 'NOT_FOUND' ? 404 : 400) });
}
