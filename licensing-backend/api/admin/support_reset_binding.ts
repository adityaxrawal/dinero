/**
 * Admin action: release a device binding on a user's behalf.
 *
 * The manual counterpart to self-service deactivation, for cases where the user
 * cannot perform it themselves -- a lost, stolen or dead machine that can no
 * longer make the call.
 *
 * A reason is mandatory and rejected if absent. This is a privileged action that
 * unbinds a paid license, so every use must leave an explanation in the audit
 * log attributable to a support decision.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import {LicensingApiError} from '../../lib/errors';
import { requirePostWithFields, handleAdminSupportError } from '../../lib/api_helpers';
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
    findFirst(args: {
      where: { accountId: string };
      orderBy: { createdAt: 'desc' };
    }): Promise<{ id: string } | null>;
    update: PrismaClient['licenseToken']['update'];
  };
  licensingAuditLog: AuditWriter;
};

/**
 * Releases a device binding on the user's behalf.
 *
 * For a lost or dead machine that can no longer deactivate itself. A reason is
 * mandatory, since this unbinds a paid licence and every use must leave an
 * attributable explanation in the audit log.
 */
export async function resetDeviceBinding(
  db: ResetBindingDb,
  input: ResetBindingInput
): Promise<ResetBindingResult> {
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

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  try {
    assertAdminAuthorized(req.headers.authorization);
    if (!requirePostWithFields(req, res, ['email'])) return;
    const { email, reason } = req.body;
    const result = await resetDeviceBinding(prisma, { email, reason });
    res.status(200).json(result);
  } catch (e) {
    handleAdminSupportError(res, e);
  }
}

export default withRequestLogging('admin/support_reset_binding', handler);
