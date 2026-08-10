/**
 * Groups spending categories for the chart, folding a long tail into 'Other'.
 *
 * A pie chart with thirty slices communicates nothing, so only the significant
 * categories keep their own slice.
 */
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
 * Groups categories for the donut, folding a long tail into 'Other'.
 *
 * A pie chart with thirty slices communicates nothing, so only significant
 * categories keep their own slice.
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
