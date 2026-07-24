import type { VercelRequest, VercelResponse } from '@vercel/node';
import type { PrismaClient } from '@prisma/client';
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

export async function getTokenAndSubscription(
  db: {
    licenseToken: {
      findFirst(args: any): Promise<any>;
      findUnique?(args: any): Promise<any>;
    };
    subscription: {
      findFirst(args: any): Promise<any>;
    };
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
