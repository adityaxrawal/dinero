/**
 * Admin plan management: list plans and edit their pricing or availability.
 *
 * Plans are data rather than code so pricing can change without a deploy. Since
 * the purchase flow reads its amount from these rows, this endpoint is
 * effectively the price control for the product and is admin-gated accordingly.
 */
import { withRequestLogging } from '../../lib/request_logging';
import type { Prisma, PrismaClient } from '@prisma/client';
import type { VercelRequest, VercelResponse } from '@vercel/node';
import { prisma } from '../../lib/db';
import { LicensingApiError, sendApiError } from '../../lib/errors';
import { assertAdminAuthorized } from '../../lib/admin_auth';
import { logAuditEvent, type AuditWriter } from '../../lib/audit';

export interface UpdatePlanInput {
  plan_id: string;
  is_active?: boolean;
  amount_minor?: number;
}

export type PlansDb = {
  plan: Pick<PrismaClient['plan'], 'findUnique' | 'update' | 'findMany'>;
  licensingAuditLog: AuditWriter;
};

/**
 * Lists plans, optionally only the active ones.
 */
export async function listPlans(db: Pick<PlansDb, 'plan'>, activeOnly: boolean) {
  return db.plan.findMany({ where: activeOnly ? { isActive: true } : undefined });
}

/**
 * Updates a plan's price or availability.
 *
 * Effectively the product's price control, since the purchase flow reads its
 * amount from these rows -- hence the admin gate on the handler.
 */
export async function updatePlan(db: PlansDb, input: UpdatePlanInput) {
  const before = await db.plan.findUnique({ where: { id: input.plan_id } });
  if (!before) {
    throw new LicensingApiError('NOT_FOUND', 'Unknown plan');
  }
  const data: Record<string, unknown> = {};
  if (input.is_active !== undefined) data.isActive = input.is_active;
  if (input.amount_minor !== undefined) data.amountMinor = input.amount_minor;

  const after = await db.plan.update({ where: { id: input.plan_id }, data });

  await logAuditEvent(db.licensingAuditLog, {
    eventType: 'plan_updated',
    payload: {
      plan_id: input.plan_id,
      before: { isActive: before.isActive, amountMinor: before.amountMinor },
      after: data,
    } as Prisma.InputJsonValue,
  });

  return after;
}

/**
 * HTTP entry point: validates the request, delegates, and maps errors to statuses.
 */
async function handler(req: VercelRequest, res: VercelResponse) {
  try {
    if (req.method === 'GET') {
      const plans = await listPlans(prisma, req.query.active_only === 'true');
      res.status(200).json({ plans });
      return;
    }
    if (req.method === 'PATCH') {
      assertAdminAuthorized(req.headers.authorization);
      const { plan_id, is_active, amount_minor } = req.body ?? {};
      if (!plan_id) {
        res.status(400).json({ code: 'VALIDATION_ERROR', message: 'plan_id is required' });
        return;
      }
      const result = await updatePlan(prisma, { plan_id, is_active, amount_minor });
      res.status(200).json(result);
      return;
    }
    res.status(405).json({ code: 'VALIDATION_ERROR', message: 'GET or PATCH only' });
  } catch (e) {
    sendApiError(res, e);
  }
}

export default withRequestLogging('admin/plans', handler);
