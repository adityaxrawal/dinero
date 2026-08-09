import { Loader2 } from 'lucide-react';
import type { SpendTrendGranularity } from '@/lib/ipc';
import type { CategoryChartSlice } from '@/components/dashboard/groupCategoriesForChart';
import { TrendChart, CategoryDonut } from './charts';
import type { useDashboardData } from './useDashboardData';

type Data = ReturnType<typeof useDashboardData>;

const GRANULARITIES: SpendTrendGranularity[] = ['daily', 'weekly', 'monthly'];

function Spinner({ className }: { className: string }) {
  return (
    <div className={`${className} flex items-center justify-center`}>
      <Loader2 className="w-4 h-4 animate-spin" style={{ color: '#064E3B' }} />
    </div>
  );
}

function GranularityToggle({
  value,
  onChange,
}: {
  value: SpendTrendGranularity;
  onChange: (g: SpendTrendGranularity) => void;
}) {
  return (
    <div
      className="flex gap-0.5 p-0.5 rounded-lg"
      style={{ background: 'rgba(6,78,59,0.06)', border: '1px solid rgba(6,78,59,0.10)' }}
      role="group"
      aria-label="Granularity"
    >
      {GRANULARITIES.map((g) => (
        <button
          key={g}
          type="button"
          onClick={() => onChange(g)}
          className="text-xs font-medium px-2.5 py-1 rounded-md transition-all"
          style={{
            background: value === g ? '#064E3B' : 'transparent',
            color: value === g ? '#F8E7C9' : '#6b8a7f',
          }}
          aria-pressed={value === g}
        >
          {g.charAt(0).toUpperCase() + g.slice(1, 3)}
        </button>
      ))}
    </div>
  );
}

function CategoryLegend({
  slices,
  onSelect,
}: {
  slices: CategoryChartSlice[];
  onSelect: (id: string) => void;
}) {
  return (
    <ul className="mt-2 space-y-1">
      {slices.slice(0, 5).map((s) => (
        <li key={s.category_id} className="flex items-center gap-2">
          <span
            className="w-2.5 h-2.5 rounded-sm flex-shrink-0"
            style={{ background: s.color }}
            aria-hidden="true"
          />
          <button
            type="button"
            className="text-xs truncate hover:underline text-left"
            style={{ color: 'var(--text-secondary)' }}
            onClick={() => onSelect(s.category_id)}
            disabled={s.category_id === '__other__'}
          >
            {s.name}
          </button>
          <span
            className="ml-auto text-xs font-medium amount"
            style={{ color: 'var(--text-primary)' }}
          >
            ₹{s.total_spend.toLocaleString()}
          </span>
        </li>
      ))}
    </ul>
  );
}

export default function ChartsRow({
  data,
  onCategoryClick,
}: {
  data: Data;
  onCategoryClick: (id: string) => void;
}) {
  return (
    <section
      aria-label="Spending analytics"
      className="grid gap-4 mb-5"
      style={{ gridTemplateColumns: '1fr 340px' }}
    >
      <div className="card-champagne p-5">
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="heading-sm">Spend Trend</h2>
            <p className="text-xs mt-0.5" style={{ color: 'var(--text-muted)' }}>
              Confirmed spend over time
            </p>
          </div>
          <GranularityToggle value={data.granularity} onChange={data.setGranularity} />
        </div>
        {data.trendLoading ? <Spinner className="h-48" /> : <TrendChart data={data.trendData ?? []} />}
      </div>

      <div className="card-champagne p-5">
        <h2 className="heading-sm mb-0.5">By Category</h2>
        <p className="text-xs mb-3" style={{ color: 'var(--text-muted)' }}>
          This month's spend
        </p>
        {data.categoriesLoading ? (
          <Spinner className="h-[220px]" />
        ) : (
          <CategoryDonut slices={data.categorySlices} onSliceClick={onCategoryClick} />
        )}
        {data.categorySlices.length > 0 && (
          <CategoryLegend slices={data.categorySlices} onSelect={onCategoryClick} />
        )}
      </div>
    </section>
  );
}
