/**
 * Starts a free trial, subject to the anti-farming eligibility rules.
 *
 * The endpoint's real work is deciding whether a trial is warranted at all. That
 * policy lives in the trial guard and is evaluated here against three facts: the
 * account's own trial history, whether the device is already bound to a
 * subscription, and whether the device has ever started a trial before.
 *
 * There are four outcomes, and the ordering of the branches below matters. A
 * recognised returning device is handled first and is not a rejection -- an
 * existing customer reinstalling receives a token for the subscription they
 * already hold, rather than being told their trial is used up. Only then are the
 * two blocking cases considered, and finally the genuine new trial.
 */
import { withRequestLogging } from '../../lib/request_logging';
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

// Trial length, mirrored in the seeded plan records. The two must agree, or
// the advertised and enforced trial periods diverge.
const TRIAL_DAYS = 14;
// Trials run on the monthly plan; converting later is a billing change rather
// than a plan migration.
const TRIAL_PLAN_ID = 'desktop_pro_monthly';

export type StartTrialDb = {
  account: Pick<PrismaClient['account'], 'findUnique' | 'create' | 'update'>;
  subscription: Pick<PrismaClient['subscription'], 'create' | 'findFirst'>;
  licenseToken: Pick<PrismaClient['licenseToken'], 'create' | 'findUnique'>;
  licensingAuditLog: AuditWriter;
};

/**
 * Evaluate eligibility and, if permitted, create the trial subscription.
 */
export async function startTrial(
  db: StartTrialDb,
  input: StartTrialInput,
  privateKeyPem: string
): Promise<StartTrialResult> {
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

  // All three inputs are gathered before the policy runs, keeping the decision
  // itself a pure function over already-known facts.
  const decision = decideTrialEligibility({
    accountTrialUsed: account?.trialUsed ?? false,
    deviceBoundSubscriptionStatus,
    deviceHasPriorTrialStartedEvent: await deviceHasPriorTrialStartedEvent(
      db.licensingAuditLog,
      input.device_id
    ),
  });
  await logTrialGuardDecision(db.licensingAuditLog, input.device_id, decision);

  // Checked first, and deliberately so: this is a paying customer reinstalling,
  // not an abuse attempt. They are handed a token for their existing
  // subscription instead of being refused a trial they no longer need.
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

  // Only now is an account created. Doing it earlier would leave an orphaned
  // account row behind for every blocked trial attempt.
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

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
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
