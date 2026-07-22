import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-OPS-006, Doc 43 §3 action 5: "Force-unbind a device from a
// license (lost/replaced Mac)... admin override of normal license_deactivate."
// Reuses the exact clearing effect `deactivateLicense` (api/license/
// deactivate.ts) already performs, keyed by account email rather than
// device_id -- the whole point of this endpoint is the case where the admin
// does NOT have the old device_id (it's gone/replaced), only the account's
// email from a support ticket. Every call requires a human-readable reason
// and is fully audited (Doc 42 §10, Doc 43 §7).
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { assertAdminAuthorized } from '../../lib/admin_auth';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface ResetBindingInput {
  email: string;
  reason: string;
}

export interface ResetBindingResult {
  status: 'binding_reset';
}

export type ResetBindingDb = {
  account: { findUnique(args: { where: { email: string } }): Promise<{ id: string } | null> };
  licenseToken: {
    findFirst(args: { where: { accountId: string }; orderBy: { createdAt: 'desc' } }): Promise<{ id: string } | null>;
    update: PrismaClient['licenseToken']['update'];
  };
  licensingAuditLog: AuditWriter;
};

export async function resetDeviceBinding(db: ResetBindingDb, input: ResetBindingInput): Promise<ResetBindingResult> {
  if (!input.reason || input.reason.trim().length === 0) {
    throw new LicensingApiError('VALIDATION_ERROR', 'reason is required to reset a device binding');
  }

  const account = await db.account.findUnique({ where: { email: input.email } });
  if (!account) {
    throw new LicensingApiError('NOT_FOUND', 'No account found for that email');
  }

  const token = await db.licenseToken.findFirst({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
  });
  if (!token) {
    throw new LicensingApiError('NOT_FOUND', 'No license token bound to this account');
  }

  await db.licenseToken.update({
    where: { id: token.id },
    data: { deviceFingerprint: null, deviceBoundAt: null, revokedAt: new Date() },
  });

  await logAuditEvent(db.licensingAuditLog, {
    accountId: account.id,
    eventType: 'admin_support_reset_binding',
    payload: { reason: input.reason },
  });

  return { status: 'binding_reset' };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  try {
    assertAdminAuthorized(req.headers.authorization);
    if (req.method !== 'POST') {
      res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
      return;
    }
    const { email, reason } = req.body ?? {};
    if (!email) {
      res.status(400).json({ code: 'VALIDATION_ERROR', message: 'email is required' });
      return;
    }
    const result = await resetDeviceBinding(prisma, { email, reason });
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(e.code === 'NOT_FOUND' ? 404 : 400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}

export default withRequestLogging('admin/support_reset_binding', handler);
