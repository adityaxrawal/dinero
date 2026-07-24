// Doc 30 TASK-BILL-001 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { updatePlan, listPlans, type PlansDb } from '../api/admin/plans';
import { SEED_PLANS } from '../prisma/seed';

function makeDb(plans: Record<string, unknown>[]): PlansDb {
  return {
    plan: {
      findUnique: vi
        .fn()
        .mockImplementation(({ where }) =>
          Promise.resolve(plans.find((p) => p.id === where.id) ?? null)
        ),
      update: vi.fn().mockImplementation(({ where, data }) => {
        // Mirrors real Prisma: update() returns a fresh row object, never
        // mutates whatever reference an earlier findUnique() call handed
        // back -- mutating in place here previously corrupted the `before`
        // snapshot updatePlan captures prior to calling update().
        const index = plans.findIndex((p) => p.id === where.id);
        plans[index] = { ...plans[index], ...data };
        return Promise.resolve(plans[index]);
      }),
      findMany: vi
        .fn()
        .mockImplementation(({ where }) =>
          Promise.resolve(where?.isActive ? plans.filter((p) => p.isActive) : plans)
        ),
    } as unknown as PlansDb['plan'],
    licensingAuditLog: {
      create: vi.fn().mockResolvedValue({}),
      findMany: vi.fn().mockResolvedValue([]),
    } as unknown as PlansDb['licensingAuditLog'],
  };
}

describe('test_plan_seeded_correctly', () => {
  it('seeds both desktop_pro_monthly (29900) and desktop_pro_annual (287040) with 14-day trials', () => {
    const monthly = SEED_PLANS.find((p) => p.id === 'desktop_pro_monthly');
    const annual = SEED_PLANS.find((p) => p.id === 'desktop_pro_annual');
    expect(monthly).toMatchObject({
      amountMinor: 29900,
      billingInterval: 'month',
      trialDays: 14,
      currency: 'INR',
    });
    expect(annual).toMatchObject({
      amountMinor: 287040,
      billingInterval: 'year',
      trialDays: 14,
      currency: 'INR',
    });
  });
});

describe('test_plan_price_change_logged_to_audit', () => {
  it('logs before/after values when amount_minor changes', async () => {
    const db = makeDb([{ id: 'desktop_pro_monthly', isActive: true, amountMinor: 29900 }]);
    await updatePlan(db, { plan_id: 'desktop_pro_monthly', amount_minor: 34900 });
    expect(db.licensingAuditLog.create).toHaveBeenCalledWith(
      expect.objectContaining({
        data: expect.objectContaining({
          eventType: 'plan_updated',
          payload: expect.objectContaining({ before: { isActive: true, amountMinor: 29900 } }),
        }),
      })
    );
  });
});

describe('test_inactive_plan_not_offered_to_new_signups', () => {
  it('listPlans(activeOnly=true) excludes a deactivated plan', async () => {
    const db = makeDb([
      { id: 'desktop_pro_monthly', isActive: true, amountMinor: 29900 },
      { id: 'desktop_pro_annual', isActive: false, amountMinor: 287040 },
    ]);
    const active = await listPlans(db, true);
    expect(active.map((p: { id: string }) => p.id)).toEqual(['desktop_pro_monthly']);
  });
});
