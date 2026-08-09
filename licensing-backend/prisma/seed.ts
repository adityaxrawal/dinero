// Doc 30 TASK-BILL-001: seeds both established plans (Doc 03 §3). Run via
// `npx prisma db seed` once a real Neon connection is configured.
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

async function main() {
  for (const plan of SEED_PLANS) {
    await prisma.plan.upsert({ where: { id: plan.id }, update: plan, create: plan });
  }
}

// Guarded so importing SEED_PLANS (e.g. from tests) never triggers a real
// DB connection attempt -- only running this file directly (`prisma db seed`) does.
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
