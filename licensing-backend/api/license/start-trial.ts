import { withRequestLogging } from '../../lib/request_logging';
// Doc 30 TASK-LIC-007: POST /api/license/start-trial
// Issued automatically during onboarding (TASK-FE-007), no credit card required.
//
// Corrected during TASK-BILL-002 (real conflict found and resolved, see
// Doc 30 changelog): no license_key -- device_id is the binding key, matching
// the corrected activate/validate/deactivate model. Real finding surfaced
// while resolving this: the already-shipped desktop trial gate
// (src-tauri/src/licensing/gate.rs::trial_days_remaining) computes the
// 14-day window purely from local_profile.created_at, entirely offline,
// with no call to this endpoint at all -- meaning TASK-BILL-009's "one
// trial per hardware UUID" is currently unenforceable (deleting and
// reinstalling the app resets the local timestamp with nothing server-side
// to catch it). This endpoint is still the right place to close that gap --
// wiring it in as a best-effort, fire-and-forget registration call at first
// launch (offline trial countdown stays local; only the abuse-tracking
// registration needs a network round-trip) is flagged as follow-up desktop
// work, not done in this pass.
//
// Device-guard logic extracted to lib/trial_guard.ts (TASK-BILL-009): a
// device already bound to an existing non-trial subscription is recognized
// as returning, not blocked as abuse -- re-issues a JWT for the existing
// subscription instead of starting a second trial.
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { requirePostWithFields } from '../../lib/api_helpers';
import { signLicenseJwt } from '../../lib/jwt';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';
import {
  decideTrialEligibility,
  logTrialGuardDecision,
  deviceHasPriorTrialStartedEvent,
} from '../../lib/trial_guard';

export interface StartTrialInput {
  email: string;
  device_id: string;
}

export type StartTrialResult =
  | { status: 'trial_started'; jwt: string; trial_ends_at: string }
  | {
      status: 'existing_subscription_recognized';
      jwt: string;
      plan: string;
      billing_interval: string;
    };

const TRIAL_DAYS = 14;
const TRIAL_PLAN_ID = 'desktop_pro_monthly';

export type StartTrialDb = {
  account: Pick<PrismaClient['account'], 'findUnique' | 'create' | 'update'>;
  subscription: Pick<PrismaClient['subscription'], 'create' | 'findFirst'>;
  licenseToken: Pick<PrismaClient['licenseToken'], 'create' | 'findUnique'>;
  licensingAuditLog: AuditWriter;
};

export async function startTrial(
  db: StartTrialDb,
  input: StartTrialInput,
  privateKeyPem: string
): Promise<StartTrialResult> {
  // Doc 30 TASK-BILL-009: is this device already bound to an account with a
  // real (non-trial) subscription? That's continuity, not abuse.
  const existingBinding = await db.licenseToken.findUnique({
    where: { deviceFingerprint: input.device_id },
  });
  let deviceBoundSubscriptionStatus: string | null = null;
  if (existingBinding) {
    const boundSub = await db.subscription.findFirst({
      where: { accountId: existingBinding.accountId },
      orderBy: { createdAt: 'desc' },
    });
    deviceBoundSubscriptionStatus = boundSub?.status ?? null;
  }

  let account = await db.account.findUnique({ where: { email: input.email } });

  const decision = decideTrialEligibility({
    accountTrialUsed: account?.trialUsed ?? false,
    deviceBoundSubscriptionStatus,
    deviceHasPriorTrialStartedEvent: await deviceHasPriorTrialStartedEvent(
      db.licensingAuditLog,
      input.device_id
    ),
  });
  await logTrialGuardDecision(db.licensingAuditLog, input.device_id, decision);

  if (decision.outcome === 'recognized_returning_device') {
    const boundSub = await db.subscription.findFirst({
      where: { accountId: existingBinding!.accountId },
      orderBy: { createdAt: 'desc' },
    });
    const jwtToken = signLicenseJwt(
      {
        sub: input.email,
        device_id: input.device_id,
        plan: boundSub!.planId,
        billing_interval: boundSub!.billingInterval,
      },
      privateKeyPem
    );
    return {
      status: 'existing_subscription_recognized',
      jwt: jwtToken,
      plan: boundSub!.planId,
      billing_interval: boundSub!.billingInterval,
    };
  }
  if (decision.outcome === 'blocked_device_reused') {
    throw new LicensingApiError(
      'VALIDATION_ERROR',
      'A trial has already been started on this device'
    );
  }
  if (decision.outcome === 'blocked_email_reused') {
    throw new LicensingApiError('VALIDATION_ERROR', 'This account has already used its trial');
  }

  if (!account) {
    account = await db.account.create({ data: { email: input.email } });
  }

  const now = new Date();
  const trialEndsAt = new Date(now.getTime() + TRIAL_DAYS * 24 * 60 * 60 * 1000);

  await db.subscription.create({
    data: {
      accountId: account.id,
      planId: TRIAL_PLAN_ID,
      status: 'trialing',
      billingInterval: 'monthly',
      currentPeriodStart: now,
      currentPeriodEnd: trialEndsAt,
    },
  });

  await db.licenseToken.create({
    data: {
      accountId: account.id,
      deviceFingerprint: input.device_id,
      deviceBoundAt: now,
      jwtIssuedAt: now,
      jwtExpiresAt: new Date(now.getTime() + 48 * 60 * 60 * 1000),
    },
  });

  await db.account.update({ where: { id: account.id }, data: { trialUsed: true } });

  await logAuditEvent(db.licensingAuditLog, {
    accountId: account.id,
    eventType: 'trial_started',
    deviceFingerprint: input.device_id,
    payload: { device_id: input.device_id },
  });

  const jwtToken = signLicenseJwt(
    {
      sub: input.email,
      device_id: input.device_id,
      plan: TRIAL_PLAN_ID,
      billing_interval: 'monthly',
    },
    privateKeyPem
  );

  return { status: 'trial_started', jwt: jwtToken, trial_ends_at: trialEndsAt.toISOString() };
}

async function handler(req: VercelRequest, res: VercelResponse) {
  if (!requirePostWithFields(req, res, ['email', 'device_id'])) return;
  const { email, device_id } = req.body;
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  if (!privateKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await startTrial(prisma, { email, device_id }, privateKeyPem);
    res.status(200).json(result);
  } catch (e) {
    sendApiError(res, e);
  }
}

export default withRequestLogging('license/start-trial', handler);
