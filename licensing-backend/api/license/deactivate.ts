// Doc 30 TASK-LIC-004: POST /api/license/deactivate
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { hashLicenseKey } from '../../lib/license_key';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';
import { consoleEmailSender, type EmailSender } from '../../lib/email';

export interface DeactivateInput {
  license_key: string;
  device_id: string;
}

export interface DeactivateResult {
  status: 'deactivated';
}

export type DeactivateDb = {
  licenseToken: Pick<PrismaClient['licenseToken'], 'findUnique' | 'update'>;
  licensingAuditLog: AuditWriter;
};

export async function deactivateLicense(
  db: DeactivateDb,
  input: DeactivateInput,
  accountEmail: string,
  emailSender: EmailSender = consoleEmailSender
): Promise<DeactivateResult> {
  const licenseKeyHash = hashLicenseKey(input.license_key);
  const token = await db.licenseToken.findUnique({ where: { licenseKeyHash } });
  if (!token) {
    throw new LicensingApiError('LICENSE_INVALID', 'Unknown license key');
  }

  // Doc 30 TASK-LIC-004: "requires the request to originate from the
  // currently-bound device" -- a stranger who merely knows the license key
  // cannot free someone else's device binding.
  if (token.deviceFingerprint !== input.device_id) {
    throw new LicensingApiError('DEVICE_MISMATCH', 'Deactivation must originate from the currently-bound device');
  }

  const now = new Date();
  // Clears the device binding (frees the license for a future activation
  // from a different hardware UUID) and marks the currently-issued JWT
  // revoked -- offline verification still trusts the signature until `exp`
  // (the hybrid model's known tradeoff), but the *next* validate/refresh
  // call will find no matching bound device and reject.
  await db.licenseToken.update({
    where: { id: token.id },
    data: { deviceFingerprint: null, revokedAt: now, deviceBoundAt: null },
  });

  await logAuditEvent(db.licensingAuditLog, {
    accountId: token.accountId,
    eventType: 'license_deactivated',
    deviceFingerprint: input.device_id,
  });

  // Doc 30 TASK-LIC-004: "sends a confirmation email to the registered
  // address as a security signal against unauthorized deactivation."
  await emailSender.send({
    to: accountEmail,
    subject: 'Your Dinero license was deactivated',
    body: `This device has been unbound from your Dinero license. If you didn't do this, contact support immediately.`,
  });

  return { status: 'deactivated' };
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
  try {
    const licenseKeyHash = hashLicenseKey(license_key);
    const token = await prisma.licenseToken.findUnique({ where: { licenseKeyHash }, include: { account: true } });
    const email = token?.account?.email ?? '';
    const result = await deactivateLicense(prisma, { license_key, device_id }, email);
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}
