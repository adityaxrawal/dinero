import type { CategorySpend } from '@/lib/ipc';
import { CATEGORICAL_PALETTE, OTHER_SLICE_COLOR } from './chartPalette';

export interface CategoryChartSlice {
  category_id: string;
  name: string;
  total_spend: number;
  color: string;
}

const MAX_SLICES = CATEGORICAL_PALETTE.length;

/**
 * TASK-FE-008: zero-spend categories are excluded from the chart (the
 * backend returns every non-deleted category, including ones with nothing
 * spent this month — useful for a budget table, meaningless as a pie
 * slice). Sorted by spend descending; a 9th+ category never gets a
 * generated hue, it folds into a single "Other" slice (dataviz skill
 * non-negotiable).
 */
export function groupCategoriesForChart(
  categories: CategorySpend[] | undefined
): CategoryChartSlice[] {
  if (!categories) return [];
  const withSpend = categories
    .filter((c) => c.total_spend > 0)
    .sort((a, b) => b.total_spend - a.total_spend);

  const top: CategoryChartSlice[] = withSpend.slice(0, MAX_SLICES).map((c, i) => ({
    category_id: c.category_id,
    name: c.name,
    total_spend: c.total_spend,
    color: CATEGORICAL_PALETTE[i],
  }));

  const rest = withSpend.slice(MAX_SLICES);
  if (rest.length > 0) {
    top.push({
      category_id: '__other__',
      name: 'Other',
      total_spend: rest.reduce((sum, c) => sum + c.total_spend, 0),
      color: OTHER_SLICE_COLOR,
    });
  }

  return top;
}
