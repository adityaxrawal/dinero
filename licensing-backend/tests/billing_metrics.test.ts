// Doc 30 TASK-BILL-007 acceptance criteria.
import { describe, it, expect, vi } from 'vitest';
import { paidMau, trialToPaidConversionRate, monthlyChurnRate, mrr, type MetricsDb } from '../lib/billing_metrics';

describe('test_paid_mau_counts_only_active_in_window', () => {
  it('counts only active-status subscriptions', async () => {
    const count = vi.fn().mockImplementation(({ where }) => Promise.resolve(where.status === 'active' ? 42 : 0));
    const db = { subscription: { count } } as unknown as Pick<MetricsDb, 'subscription'>;
    expect(await paidMau(db)).toBe(42);
    expect(count).toHaveBeenCalledWith({ where: { status: 'active' } });
  });
});

describe('test_conversion_rate_calculation', () => {
  it('computes converted/started within the window', async () => {
    const count = vi.fn().mockImplementation(({ where }) => Promise.resolve(where.status === 'active' ? 3 : 10));
    const db = { subscription: { count } } as unknown as Pick<MetricsDb, 'subscription'>;
    expect(await trialToPaidConversionRate(db, 90)).toBeCloseTo(0.3);
  });

  it('returns 0 when no trials started (avoids divide-by-zero)', async () => {
    const count = vi.fn().mockResolvedValue(0);
    const db = { subscription: { count } } as unknown as Pick<MetricsDb, 'subscription'>;
    expect(await trialToPaidConversionRate(db, 90)).toBe(0);
  });
});

describe('test_churn_rate_calculation', () => {
  it('computes churned/active-at-start', async () => {
    const count = vi.fn().mockImplementation(({ where }) => Promise.resolve(where.status === 'canceled' ? 5 : 100));
    const db = { subscription: { count } } as unknown as Pick<MetricsDb, 'subscription'>;
    expect(await monthlyChurnRate(db)).toBeCloseTo(0.05);
  });
});

describe('test_mrr_calculation_matches_target_formula', () => {
  it('sums amountMinor across active subscriptions grouped by plan, divided by 100', async () => {
    const db: MetricsDb = {
      subscription: {
        findMany: vi.fn().mockResolvedValue([{ planId: 'desktop_pro_monthly' }, { planId: 'desktop_pro_monthly' }, { planId: 'desktop_pro_annual' }]),
      } as unknown as MetricsDb['subscription'],
      plan: {
        findUnique: vi.fn().mockImplementation(({ where }) =>
          Promise.resolve(where.id === 'desktop_pro_monthly' ? { amountMinor: 29900 } : { amountMinor: 287040 })
        ),
      } as unknown as MetricsDb['plan'],
    };
    // 29900*2 + 287040 = 346840 minor -> 3468.40
    expect(await mrr(db)).toBeCloseTo(3468.4);
  });
});
