/**
 * Chart components for the dashboard.
 *
 * Colours come from the shared palette rather than being chosen per chart, which
 * is what keeps series distinguishable and accessible across every chart.
 */
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  ResponsiveContainer,
  PieChart,
  Pie,
  Cell,
} from 'recharts';
import type { SpendTrendPoint } from '@/lib/ipc';
import type { CategoryChartSlice } from '@/components/dashboard/groupCategoriesForChart';
import { SEQUENTIAL_LINE_COLOR } from '@/components/dashboard/chartPalette';

type RechartsValue = string | number | readonly (string | number)[] | undefined;

const TOOLTIP_STYLE = {
  background: 'hsl(38, 55%, 91%)',
  border: '1px solid #d9c8a8',
  borderRadius: 8,
  fontSize: 12,
  color: '#0d2b22',
};

/** Formats an amount as rupees for chart axes and tooltips. */
const rupees = (value: RechartsValue) => `₹ ${Number(value).toLocaleString()}`;

/** Spend-over-time line chart. */
export function TrendChart({ data }: { data: SpendTrendPoint[] }) {
  if (!data || data.length === 0) {
    return (
      <div
        className="h-48 flex items-center justify-center text-sm"
        style={{ color: 'var(--text-muted)' }}
      >
        No spend recorded in this window yet.
      </div>
    );
  }
  return (
    <div className="h-48" role="img" aria-label="Line chart of spend over time">
      <ResponsiveContainer width="100%" height="100%">
        <LineChart data={data} margin={{ top: 4, right: 8, left: -20, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="rgba(6,78,59,0.10)" vertical={false} />
          <XAxis
            dataKey="period"
            tick={{ fontSize: 10, fill: '#6b8a7f' }}
            axisLine={false}
            tickLine={false}
          />
          <YAxis
            tick={{ fontSize: 10, fill: '#6b8a7f' }}
            axisLine={false}
            tickLine={false}
            width={52}
          />
          <Tooltip
            formatter={(value: RechartsValue) => [rupees(value), 'Spend']}
            contentStyle={TOOLTIP_STYLE}
          />
          <Line
            type="monotone"
            dataKey="total_spend"
            stroke={SEQUENTIAL_LINE_COLOR}
            strokeWidth={2}
            dot={{ r: 3, fill: SEQUENTIAL_LINE_COLOR }}
            activeDot={{ r: 5, fill: '#064E3B' }}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}

interface DonutSliceClick {
  category_id?: string;
  payload?: { category_id?: string };
}

/** Category breakdown donut, with direct labels. */
export function CategoryDonut({
  slices,
  onSliceClick,
}: {
  slices: CategoryChartSlice[];
  onSliceClick: (id: string) => void;
}) {
  if (slices.length === 0) {
    return (
      <div
        className="h-full flex items-center justify-center text-sm"
        style={{ color: 'var(--text-muted)' }}
      >
        No spend yet this month.
      </div>
    );
  }
  return (
    <div style={{ height: 220 }} role="img" aria-label="Donut chart of spend by category">
      <ResponsiveContainer width="100%" height="100%">
        <PieChart>
          <Pie
            data={slices}
            dataKey="total_spend"
            nameKey="name"
            innerRadius="52%"
            outerRadius="78%"
            paddingAngle={2}
            onClick={(entry: DonutSliceClick) =>
              onSliceClick(entry.payload?.category_id ?? entry.category_id ?? '')
            }
            cursor="pointer"
          >
            {slices.map((slice) => (
              <Cell key={slice.category_id} fill={slice.color} />
            ))}
          </Pie>
          <Tooltip
            formatter={(value: RechartsValue) => [rupees(value), '']}
            contentStyle={TOOLTIP_STYLE}
          />
        </PieChart>
      </ResponsiveContainer>
    </div>
  );
}
