import { withRequestLogging } from '../../lib/request_logging';
import { handleAdminSupportError } from '../../lib/api_helpers';
// Doc 30 TASK-OPS-006, Doc 43 §3 action 7: "Look up an account by redacted
// email or license key, view activation/validation/refresh history."
// Internal-only endpoint, admin-key-authenticated, read-only. Reuses the
// existing accounts/license_tokens/licensing_audit_log tables -- this
// backend holds identity/billing data only, never financial content, so
// there is nothing further to redact in the history itself, but the email
// in the response is still masked (see maskEmail) since even the one
// authenticated admin has no operational need for the full address here.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import {LicensingApiError} from '../../lib/errors';
import { assertAdminAuthorized } from '../../lib/admin_auth';
import { maskEmail } from '../../lib/license_key';
import { recommendRecovery, type SupportCaseType } from '../../lib/support_recovery';

export interface SupportLookupInput {
  email: string;
  case_type?: SupportCaseType;
}

type AccountWithBindings = {
  id: string;
  email: string;
  trialUsed: boolean;
  subscriptions: { status: string; planId: string; currentPeriodEnd: Date | null }[];
  licenseTokens: {
    deviceFingerprint: string | null;
    jwtIssuedAt: Date | null;
    jwtExpiresAt: Date | null;
    revokedAt: Date | null;
  }[];
};

export type SupportLookupDb = {
  account: {
    findUnique(args: {
      where: { email: string };
      include: { subscriptions: true; licenseTokens: true };
    }): Promise<AccountWithBindings | null>;
  };
  licensingAuditLog: Pick<PrismaClient['licensingAuditLog'], 'findMany'>;
};

export interface SupportLookupResult {
  account_id: string;
  email_masked: string;
  trial_used: boolean;
  subscriptions: { status: string; plan_id: string; current_period_end: string | null }[];
  license_tokens: {
    device_bound: boolean;
    jwt_issued_at: string | null;
    jwt_expires_at: string | null;
    revoked_at: string | null;
  }[];
  history: { event_type: string; created_at: string }[];
  recommended_recovery?: ReturnType<typeof recommendRecovery>;
}

export async function lookupSupportAccount(
  db: SupportLookupDb,
  input: SupportLookupInput
): Promise<SupportLookupResult> {
  const account = await db.account.findUnique({
    where: { email: input.email },
    include: { subscriptions: true, licenseTokens: true },
  });

  if (!account) {
    throw new LicensingApiError('NOT_FOUND', 'No account found for that email');
  }

  const historyRows = await db.licensingAuditLog.findMany({
    where: { accountId: account.id },
    orderBy: { createdAt: 'desc' },
    take: 20,
  });

  return {
    account_id: account.id,
    email_masked: maskEmail(account.email),
    trial_used: account.trialUsed,
    subscriptions: account.subscriptions.map((s) => ({
      status: s.status,
      plan_id: s.planId,
      current_period_end: s.currentPeriodEnd ? s.currentPeriodEnd.toISOString() : null,
    })),
    license_tokens: account.licenseTokens.map((t) => ({
      device_bound: t.deviceFingerprint !== null,
      jwt_issued_at: t.jwtIssuedAt ? t.jwtIssuedAt.toISOString() : null,
      jwt_expires_at: t.jwtExpiresAt ? t.jwtExpiresAt.toISOString() : null,
      revoked_at: t.revokedAt ? t.revokedAt.toISOString() : null,
    })),
    history: historyRows.map((h) => ({
      event_type: h.eventType,
      created_at: h.createdAt.toISOString(),
    })),
    recommended_recovery: input.case_type ? recommendRecovery(input.case_type) : undefined,
  };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  try {
    assertAdminAuthorized(req.headers.authorization);
    if (req.method !== 'GET') {
      res.status(405).json({ code: 'VALIDATION_ERROR', message: 'GET only' });
      return;
    }
    const email = typeof req.query.email === 'string' ? req.query.email : undefined;
    if (!email) {
      res.status(400).json({ code: 'VALIDATION_ERROR', message: 'email is required' });
      return;
    }
    const caseType =
      typeof req.query.case_type === 'string'
        ? (req.query.case_type as SupportCaseType)
        : undefined;
    const result = await lookupSupportAccount(prisma, { email, case_type: caseType });
    res.status(200).json(result);
  } catch (e) {
    handleAdminSupportError(res, e);
  }
}

export default withRequestLogging('admin/support_lookup', handler);
