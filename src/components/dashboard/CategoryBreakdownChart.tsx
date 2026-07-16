import { useNavigate } from 'react-router-dom';
import { PieChart, Pie, Cell, Tooltip, Legend, ResponsiveContainer } from 'recharts';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import type { CategorySpend } from '@/lib/ipc';
import { groupCategoriesForChart, type CategoryChartSlice } from './groupCategoriesForChart';

interface CategoryBreakdownChartProps {
  categories: CategorySpend[] | undefined;
  isLoading: boolean;
}

/**
 * TASK-FE-008 (Doc 30): pie/donut breakdown of this month's spend by
 * category. Clickable segments navigate to a category-filtered
 * transactions list (`/#/transactions?category=<id>`) — TASK-FE-009 reads
 * that query param. "Other" (folded 9th+ categories) isn't clickable, since
 * it doesn't map to a single filterable category.
 */
export default function CategoryBreakdownChart({ categories, isLoading }: CategoryBreakdownChartProps) {
  const navigate = useNavigate();
  const slices = groupCategoriesForChart(categories);

  const handleSliceClick = (categoryId: string) => {
    if (categoryId === '__other__') return;
    navigate(`/transactions?category=${encodeURIComponent(categoryId)}`);
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Spend by Category</CardTitle>
        <CardDescription>This month's confirmed spend, broken down by category.</CardDescription>
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <div className="h-64 flex items-center justify-center text-sm text-muted-foreground" role="status">
            Loading…
          </div>
        ) : slices.length === 0 ? (
          <div className="h-64 flex items-center justify-center text-sm text-muted-foreground" role="status">
            No categorized spend yet this month.
          </div>
        ) : (
          <div className="h-64" role="img" aria-label="Pie chart of spend by category">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  data={slices}
                  dataKey="total_spend"
                  nameKey="name"
                  innerRadius="55%"
                  outerRadius="80%"
                  paddingAngle={2}
                  // Dataviz skill relief rule: 3 of the 8 categorical slots
                  // fall below 3:1 contrast on a light surface, so slices
                  // always carry a visible direct label, not color alone.
                  label={({ name, percent }) => `${name} ${((percent ?? 0) * 100).toFixed(0)}%`}
                  onClick={(entry: unknown) => handleSliceClick((entry as CategoryChartSlice).category_id)}
                  cursor="pointer"
                >
                  {slices.map((slice) => (
                    <Cell key={slice.category_id} fill={slice.color} />
                  ))}
                </Pie>
                <Tooltip formatter={(value) => `₹ ${Number(value).toLocaleString()}`} />
                <Legend />
              </PieChart>
            </ResponsiveContainer>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
