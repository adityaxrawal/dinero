/**
 * Seeds the subscription plans a fresh database needs to function.
 *
 * Written as upserts rather than inserts so the script is idempotent and safe to
 * re-run against an existing database -- a redeploy must not fail on plans that
 * already exist, nor duplicate them.
 *
 * Prices are integer minor units (paise), matching the convention used
 * throughout the codebase to keep money arithmetic exact.
 */
import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

export const SEED_PLANS = [
  {
    id: 'desktop_pro_monthly',
    name: 'Dinero Pro (Monthly)',
    currency: 'INR',
    amountMinor: 29900,
    billingInterval: 'month',
    trialDays: 14,
    isActive: true,
  },
  {
    id: 'desktop_pro_annual',
    name: 'Dinero Pro (Annual)',
    currency: 'INR',
    amountMinor: 287040,
    billingInterval: 'year',
    trialDays: 14,
    isActive: true,
  },
];

/**
 * Upserts the seed plans, idempotently.
 */
async function main() {
  for (const plan of SEED_PLANS) {
    await prisma.plan.upsert({ where: { id: plan.id }, update: plan, create: plan });
  }
}

if (require.main === module) {
  main()
    .catch((e) => {
      console.error(e);
      process.exitCode = 1;
    })
    .finally(async () => {
      await prisma.$disconnect();
    });
}
