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
import type { PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError } from '../../lib/errors';
import { signLicenseJwt } from '../../lib/jwt';
import { logAuditEvent, countRecentEvents, type AuditWriter } from '../../lib/audit';

export interface StartTrialInput {
  email: string;
  device_id: string;
}

export interface StartTrialResult {
  status: 'trial_started';
  jwt: string;
  trial_ends_at: string;
}

const TRIAL_DAYS = 14;
const TRIAL_PLAN_ID = 'desktop_pro_monthly';

export type StartTrialDb = {
  account: Pick<PrismaClient['account'], 'findUnique' | 'create' | 'update'>;
  subscription: Pick<PrismaClient['subscription'], 'create'>;
  licenseToken: Pick<PrismaClient['licenseToken'], 'create'>;
  licensingAuditLog: AuditWriter;
};

export async function startTrial(db: StartTrialDb, input: StartTrialInput, privateKeyPem: string): Promise<StartTrialResult> {
  // Doc 30 TASK-LIC-007: "checking for a prior trial against the same
  // hardware UUID to prevent reinstall-based trial abuse" -- device-fingerprint
  // check lives here; TASK-BILL-009 later adds the combined email+device
  // guard with the "OS reinstall, not abuse" carve-out on top of this.
  const priorDeviceTrials = await countRecentEvents(
    db.licensingAuditLog,
    'trial_started',
    Number.MAX_SAFE_INTEGER,
    (payload) => (payload as { device_id?: string } | null)?.device_id === input.device_id
  );
  if (priorDeviceTrials > 0) {
    throw new LicensingApiError('VALIDATION_ERROR', 'A trial has already been started on this device');
  }

  let account = await db.account.findUnique({ where: { email: input.email } });
  if (!account) {
    account = await db.account.create({ data: { email: input.email } });
  } else if (account.trialUsed) {
    throw new LicensingApiError('VALIDATION_ERROR', 'This account has already used its trial');
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
    { sub: input.email, device_id: input.device_id, plan: TRIAL_PLAN_ID, billing_interval: 'monthly' },
    privateKeyPem
  );

  return { status: 'trial_started', jwt: jwtToken, trial_ends_at: trialEndsAt.toISOString() };
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  if (req.method !== 'POST') {
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'POST only' });
    return;
  }
  const { email, device_id } = req.body ?? {};
  if (!email || !device_id) {
    res.status(400).json({ code: 'VALIDATION_ERROR', message: 'email and device_id are required' });
    return;
  }
  const privateKeyPem = process.env.JWT_PRIVATE_KEY_PEM;
  if (!privateKeyPem) {
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Server misconfigured' });
    return;
  }
  try {
    const result = await startTrial(prisma, { email, device_id }, privateKeyPem);
    res.status(200).json(result);
  } catch (e) {
    if (e instanceof LicensingApiError) {
      res.status(400).json({ code: e.code, message: e.message });
      return;
    }
    res.status(500).json({ code: 'INTERNAL_ERROR', message: 'Unexpected error' });
  }
}
