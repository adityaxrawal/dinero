import { describe, it, expect } from 'vitest';
import { computeMonthOverMonthDelta } from './computeMonthOverMonthDelta';

describe('computeMonthOverMonthDelta', () => {
  it('returns null with fewer than 2 points', () => {
    expect(computeMonthOverMonthDelta(undefined)).toBeNull();
    expect(computeMonthOverMonthDelta([])).toBeNull();
    expect(computeMonthOverMonthDelta([{ period: '2026-06', total_spend: 100 }])).toBeNull();
  });

  it('returns null when the prior month had zero spend (undefined percentage change)', () => {
    expect(
      computeMonthOverMonthDelta([
        { period: '2026-05', total_spend: 0 },
        { period: '2026-06', total_spend: 500 },
      ])
    ).toBeNull();
  });

  it('computes a positive percentage increase', () => {
    const delta = computeMonthOverMonthDelta([
      { period: '2026-05', total_spend: 100 },
      { period: '2026-06', total_spend: 150 },
    ]);
    expect(delta).toBe(50);
  });

  it('computes a negative percentage decrease', () => {
    const delta = computeMonthOverMonthDelta([
      { period: '2026-05', total_spend: 200 },
      { period: '2026-06', total_spend: 150 },
    ]);
    expect(delta).toBe(-25);
  });

  it('sorts out-of-order input by period before comparing the last two', () => {
    const delta = computeMonthOverMonthDelta([
      { period: '2026-06', total_spend: 150 },
      { period: '2026-04', total_spend: 999 },
      { period: '2026-05', total_spend: 100 },
    ]);
    expect(delta).toBe(50);
  });
});
