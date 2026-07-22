// Doc 30 TASK-LIC-004: POST /api/license/deactivate
//
// Corrected during TASK-BILL-002 (real conflict found and resolved, see
// Doc 30 changelog): matches the already-shipped desktop client exactly --
// reuses the same `{ device_id }` shape as validate (`ValidateRequest` is
// literally reused for both calls in `licensing/client.rs`), no license_key.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';
import { consoleEmailSender, type EmailSender } from '../../lib/email';

export interface DeactivateInput {
  device_id: string;
}

export interface DeactivateResult {
  status: 'deactivated';
}

export type DeactivateDb = {
  licenseToken: {
    findUnique(args: { where: { deviceFingerprint: string }; include: { account: true } }): Promise<{ id: string; accountId: string; account: { email: string } } | null>;
    update: PrismaClient['licenseToken']['update'];
  };
  licensingAuditLog: AuditWriter;
};

export async function deactivateLicense(db: DeactivateDb, input: DeactivateInput, emailSender: EmailSender = consoleEmailSender): Promise<DeactivateResult> {
  const token = await db.licenseToken.findUnique({ where: { deviceFingerprint: input.device_id }, include: { account: true } });
  if (!token) {
    throw new LicensingApiError('LICENSE_INVALID', 'No license bound to this device');
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
    to: token.account.email,
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
  const { device_id } = req.body ?? {};
  if (!device_id) {
    res.status(400).json({ code: 'VALIDATION_ERROR', message: 'device_id is required' });
    return;
  }
  try {
    const result = await deactivateLicense(prisma, { device_id });
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}
