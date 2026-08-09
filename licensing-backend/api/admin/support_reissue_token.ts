import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-OPS-006: "reissuing a token after a hardware change." Unlike
// the self-service /api/license/refresh-token, this does not require a
// currently-valid JWT from the requesting device at all -- it exists
// specifically for the case where the old device is gone (replaced Mac,
// reinstalled OS wiping the old install) and the user has no token left to
// refresh with. Binds a new device_id directly and issues a fresh JWT.
// Requires a reason and is fully audited, same as support_reset_binding.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import {LicensingApiError} from '../../lib/errors';
import { requirePostWithFields, handleAdminSupportError } from '../../lib/api_helpers';
import { assertAdminAuthorized } from '../../lib/admin_auth';
import { signLicenseJwt } from '../../lib/jwt';
import { maskDeviceFingerprint } from '../../lib/license_key';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface ReissueTokenInput {
  email: string;
  new_device_id: string;
  reason: string;
}

export interface ReissueTokenResult {
  status: 'reissued';
  jwt: string;
  expires_at: string;
}

export type ReissueTokenDb = {
  account: { findUnique(args: { where: { email: string } }): Promise<{ id: string } | null> };
  subscription: {
    findFirst(args: {
      where: { accountId: string };
      orderBy: { createdAt: 'desc' };
    }): Promise<{ planId: string; billingInterval: string; status: string } | null>;
  };
  licenseToken: {
    findFirst(args: {
      where: { accountId?: string; deviceFingerprint?: string };
      orderBy?: { createdAt: 'desc' };
    }): Promise<{ id: string; accountId: string } | null>;
    update: PrismaClient['licenseToken']['update'];
    create: PrismaClient['licenseToken']['create'];
  };
  licensingAuditLog: AuditWriter;
};

export async function reissueToken(
  db: ReissueTokenDb,
  input: ReissueTokenInput,
  privateKeyPem: string
): Promise<ReissueTokenResult> {
  if (!input.reason || input.reason.trim().length === 0) {
    throw new LicensingApiError('VALIDATION_ERROR', 'reason is required to reissue a token');
  }

  const account = await db.account.findUnique({ where: { email: input.email } });
  if (!account) {
    throw new LicensingApiError('NOT_FOUND', 'No account found for that email');
  }

  const conflicting = await db.licenseToken.findFirst({
    where: { deviceFingerprint: input.new_device_id },
  });
  if (conflicting && conflicting.accountId !== account.id) {
    throw new LicensingApiError(
      'DEVICE_ALREADY_BOUND',
      `That device is already bound to another account's license (${maskDeviceFingerprint(input.new_device_id)})`
    );
  }

  const subscription = await db.subscription.findFirst({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
  });
  if (!subscription || !['trialing', 'active', 'past_due'].includes(subscription.status)) {
    throw new LicensingApiError('LICENSE_INVALID', 'No refreshable subscription for this account');
  }

  const now = new Date();
  const expiresAt = new Date(now.getTime() + 48 * 60 * 60 * 1000);
  const newJwt = signLicenseJwt(
    {
      sub: input.email,
      device_id: input.new_device_id,
      plan: subscription.planId,
      billing_interval: subscription.billingInterval,
    },
    privateKeyPem
  );

  const existing = await db.licenseToken.findFirst({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
  });
  if (existing) {
    await db.licenseToken.update({
      where: { id: existing.id },
      data: {
        deviceFingerprint: input.new_device_id,
        deviceBoundAt: now,
        jwtIssuedAt: now,
        jwtExpiresAt: expiresAt,
        revokedAt: null,
      },
    });
  } else {
    await db.licenseToken.create({
      data: {
        accountId: account.id,
        deviceFingerprint: input.new_device_id,
        deviceBoundAt: now,
        jwtIssuedAt: now,
        jwtExpiresAt: expiresAt,
      },
    });
  }

  await logAuditEvent(db.licensingAuditLog, {
    accountId: account.id,
    eventType: 'admin_support_reissue_token',
    deviceFingerprint: input.new_device_id,
    payload: { reason: input.reason },
  });

  return { status: 'reissued', jwt: newJwt, expires_at: expiresAt.toISOString() };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  try {
    assertAdminAuthorized(req.headers.authorization);
    if (!requirePostWithFields(req, res, ['email', 'new_device_id'])) return;
    const { email, new_device_id, reason } = req.body;
    const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
    if (!privateKeyPem) {
      res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
      return;
    }
    const result = await reissueToken(prisma, { email, new_device_id, reason }, privateKeyPem);
    res.status(200).json(result);
  } catch (e) {
    handleAdminSupportError(res, e);
  }
}

export default withRequestLogging('admin/support_reissue_token', handler);
