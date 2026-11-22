/**
 * Device deactivation: releases the license binding so another machine can claim it.
 *
 * The user-facing escape hatch from one-device-per-license. Without it, a lost
 * or replaced machine would strand the license permanently and force every
 * hardware change through support.
 *
 * Deactivation revokes the token rather than deleting the row, so the audit
 * trail of what was bound when survives.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields, assertDeviceProof } from '../../lib/api_helpers';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';
import { consoleEmailSender, type EmailSender } from '../../lib/email';

export interface DeactivateInput {
  device_id: string;
  /** The device's current (possibly expired) license token, as proof. */
  jwt: string;
}

export interface DeactivateResult {
  status: 'deactivated';
}

export type DeactivateDb = {
  licenseToken: {
    findUnique(args: {
      where: { deviceFingerprint: string };
      include: { account: true };
    }): Promise<{ id: string; accountId: string; account: { email: string } } | null>;
    update: PrismaClient['licenseToken']['update'];
  };
  licensingAuditLog: AuditWriter;
};

/**
 * Revoke this device's token and record the deactivation.
 */
export async function deactivateLicense(
  db: DeactivateDb,
  input: DeactivateInput,
  publicKeyPem: string,
  emailSender: EmailSender = consoleEmailSender
): Promise<DeactivateResult> {
  assertDeviceProof(input.jwt, input.device_id, publicKeyPem);
  const token = await db.licenseToken.findUnique({
    where: { deviceFingerprint: input.device_id },
    include: { account: true },
  });
  if (!token) {
    throw new LicensingApiError('LICENSE_INVALID', 'No license bound to this device');
  }

  const now = new Date();
  await db.licenseToken.update({
    where: { id: token.id },
    data: { deviceFingerprint: null, revokedAt: now, deviceBoundAt: null },
  });

  await logAuditEvent(db.licensingAuditLog, {
    accountId: token.accountId,
    eventType: 'license_deactivated',
    deviceFingerprint: input.device_id,
  });

  // Device-only trial accounts have a placeholder .invalid address (M15).
  if (token.account.email.endsWith('.invalid')) return { status: 'deactivated' };
  await emailSender.send({
    to: token.account.email,
    subject: 'Your Dinero license was deactivated',
    body: `This device has been unbound from your Dinero license. If you didn't do this, contact support immediately.`,
  });

  return { status: 'deactivated' };
}

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  if (!requirePostWithFields(req, res, ['device_id', 'jwt'])) return;
  const { device_id, jwt } = req.body;
  const publicKeyPem = process.env.JWT_PUBLIC_KEY_PEM;
  if (!publicKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await deactivateLicense(prisma, { device_id, jwt }, publicKeyPem);
    res.status(200).json(result);
  } catch (e) {
    sendApiError(res, e);
  }
}

export default withRequestLogging('license/deactivate', handler);
