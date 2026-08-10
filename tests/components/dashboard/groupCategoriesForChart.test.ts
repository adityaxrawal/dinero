import { describe, it, expect } from 'vitest';
import { groupCategoriesForChart } from '@/components/dashboard/groupCategoriesForChart';
import type { CategorySpend } from '@/lib/ipc';

function cat(id: string, name: string, spend: number): CategorySpend {
  return {
    category_id: id,
    name,
    total_spend: spend,
    monthly_budget: null,
    utilization_pct: 0,
    currency: 'INR',
  };
}

describe('groupCategoriesForChart', () => {
  it('returns an empty array for undefined/empty input', () => {
    expect(groupCategoriesForChart(undefined)).toEqual([]);
    expect(groupCategoriesForChart([])).toEqual([]);
  });

  it('excludes zero-spend categories', () => {
    const result = groupCategoriesForChart([cat('a', 'Food', 100), cat('b', 'Empty', 0)]);
    expect(result.map((s) => s.category_id)).toEqual(['a']);
  });

  it('sorts by spend descending and assigns distinct palette colors', () => {
    const result = groupCategoriesForChart([cat('a', 'Small', 10), cat('b', 'Big', 100)]);
    expect(result.map((s) => s.category_id)).toEqual(['b', 'a']);
    expect(result[0].color).not.toBe(result[1].color);
  });

  it('folds a 9th+ category into a single "Other" slice rather than a generated hue', () => {
    const categories = Array.from({ length: 10 }, (_, i) => cat(`c${i}`, `Cat ${i}`, 10 - i));
    const result = groupCategoriesForChart(categories);
    expect(result).toHaveLength(9);
    const other = result[result.length - 1];
    expect(other.category_id).toBe('__other__');
    // The 2 smallest-spend categories (indices 8, 9 -> spend 2, 1) fold in.
    expect(other.total_spend).toBe(2 + 1);
  });
});
