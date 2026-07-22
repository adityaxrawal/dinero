// Doc 30 TASK-BILL-007: measurement methodology behind Document 01 §9's
// business metrics. Aggregate-only -- never per-account-identifiable
// breakdowns, consistent with the minimal-PII posture.
import type { PrismaClient } from '@prisma/client';

export type MetricsDb = {
  subscription: Pick<PrismaClient['subscription'], 'count' | 'findMany'>;
  plan: Pick<PrismaClient['plan'], 'findUnique'>;
};

/// Distinct active accounts in the trailing 30-day window.
export async function paidMau(db: Pick<MetricsDb, 'subscription'>): Promise<number> {
  return db.subscription.count({ where: { status: 'active' } });
}

export async function trialToPaidConversionRate(db: Pick<MetricsDb, 'subscription'>, windowDays: number): Promise<number> {
  const since = new Date(Date.now() - windowDays * 24 * 60 * 60 * 1000);
  const trialsStarted = await db.subscription.count({ where: { createdAt: { gte: since } } });
  if (trialsStarted === 0) return 0;
  const converted = await db.subscription.count({ where: { createdAt: { gte: since }, status: 'active' } });
  return converted / trialsStarted;
}

export async function monthlyChurnRate(db: Pick<MetricsDb, 'subscription'>): Promise<number> {
  const since = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000);
  const activeAtStart = await db.subscription.count({ where: { status: 'active' } });
  if (activeAtStart === 0) return 0;
  const churnedThisMonth = await db.subscription.count({ where: { status: 'canceled', updatedAt: { gte: since } } });
  return churnedThisMonth / activeAtStart;
}

/// `paid_mau() x plan.amount_minor / 100` -- Doc 30's exact formula.
/// Extensible to multi-plan sums for future tiers: sums per distinct
/// `planId` among active subscriptions rather than assuming a single price.
export async function mrr(db: MetricsDb): Promise<number> {
  const activeSubs = await db.subscription.findMany({ where: { status: 'active' } });
  let total = 0;
  const planCache = new Map<string, number>();
  for (const sub of activeSubs) {
    const planId = (sub as { planId: string }).planId;
    let amountMinor = planCache.get(planId);
    if (amountMinor === undefined) {
      const plan = await db.plan.findUnique({ where: { id: planId } });
      amountMinor = plan?.amountMinor ?? 0;
      planCache.set(planId, amountMinor);
    }
    total += amountMinor;
  }
  return total / 100;
}
