/**
 * Revenue and retention metrics computed from subscription rows.
 *
 * Read by the admin metrics endpoint. Every function takes a narrowed database
 * shape rather than the full client, which keeps them callable with a small fake
 * in tests and makes each one's data access explicit in its signature.
 *
 * Rates return 0 rather than NaN on an empty denominator, so a new deployment
 * with no subscriptions yet reports zeroes instead of broken numbers.
 */
import type { PrismaClient } from '@prisma/client';

export type MetricsDb = {
  subscription: Pick<PrismaClient['subscription'], 'count' | 'findMany'>;
  plan: Pick<PrismaClient['plan'], 'findUnique'>;
};

/**
 * Count of currently active paid subscriptions.
 */
export async function paidMau(db: Pick<MetricsDb, 'subscription'>): Promise<number> {
  return db.subscription.count({ where: { status: 'active' } });
}

/**
 * Proportion of trials started in the window that are now active.
 */
export async function trialToPaidConversionRate(
  db: Pick<MetricsDb, 'subscription'>,
  windowDays: number
): Promise<number> {
  const since = new Date(Date.now() - windowDays * 24 * 60 * 60 * 1000);
  const trialsStarted = await db.subscription.count({ where: { createdAt: { gte: since } } });
  if (trialsStarted === 0) return 0;
  const converted = await db.subscription.count({
    where: { createdAt: { gte: since }, status: 'active' },
  });
  return converted / trialsStarted;
}

/**
 * Cancellations in the last 30 days over the active base.
 */
export async function monthlyChurnRate(db: Pick<MetricsDb, 'subscription'>): Promise<number> {
  const since = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000);
  const activeAtStart = await db.subscription.count({ where: { status: 'active' } });
  if (activeAtStart === 0) return 0;
  const churnedThisMonth = await db.subscription.count({
    where: { status: 'canceled', updatedAt: { gte: since } },
  });
  return churnedThisMonth / activeAtStart;
}

/**
 * Monthly recurring revenue across active subscriptions.
 *
 * Plan prices are cached per id, so a few hundred subscriptions on a handful of
 * plans do not produce a query each.
 */
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
