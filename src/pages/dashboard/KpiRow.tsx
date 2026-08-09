import { TrendingUp, TrendingDown, ArrowUpRight, ArrowDownRight } from 'lucide-react';

function KpiTile({
  label,
  value,
  valueColor,
  delta,
  deltaLabel,
  icon,
  iconBg,
}: {
  label: string;
  value: string;
  valueColor?: string;
  delta?: number | null;
  deltaLabel?: string;
  icon: React.ReactNode;
  iconBg: string;
}) {
  return (
    <div className="kpi-tile flex-1 min-w-0">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p className="kpi-label">{label}</p>
          <p className="kpi-value mt-1.5" style={{ color: valueColor ?? 'var(--text-primary)' }}>
            {value}
          </p>
          {delta != null && (
            <p className="kpi-delta" style={{ color: delta > 0 ? '#ef4444' : '#10b981' }}>
              {delta > 0 ? (
                <TrendingUp className="w-3 h-3" aria-hidden="true" />
              ) : (
                <TrendingDown className="w-3 h-3" aria-hidden="true" />
              )}
              {Math.abs(delta).toFixed(1)}% {deltaLabel ?? 'vs last month'}
            </p>
          )}
        </div>
        <div
          className="flex items-center justify-center rounded-xl flex-shrink-0"
          style={{ width: 36, height: 36, backgroundColor: iconBg }}
          aria-hidden="true"
        >
          {icon}
        </div>
      </div>
    </div>
  );
}

function LimitBar({ spent, limit }: { spent: number; limit: number }) {
  const pct = limit > 0 ? Math.min(100, (spent / limit) * 100) : 0;
  const color = pct > 90 ? '#ef4444' : pct > 75 ? '#f59e0b' : '#064E3B';
  return (
    <div className="kpi-tile flex-1 min-w-0">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <p className="kpi-label">Monthly Limit</p>
          <p className="kpi-value mt-1.5" style={{ color: color }}>
            {pct.toFixed(0)}%
          </p>
          <p className="kpi-delta" style={{ color: 'var(--text-muted)' }}>
            ₹{spent.toLocaleString()} of ₹{limit.toLocaleString()}
          </p>
        </div>
      </div>
      <div className="mt-3">
        <div
          className="w-full h-1.5 rounded-full overflow-hidden"
          style={{ background: 'rgba(6,78,59,0.10)' }}
          role="progressbar"
          aria-valuenow={Math.round(pct)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={`${Math.round(pct)}% of monthly limit used`}
        >
          <div
            className="h-full rounded-full transition-all duration-500"
            style={{ width: `${pct}%`, backgroundColor: color }}
          />
        </div>
      </div>
    </div>
  );
}

const RED = '#ef4444';
const GREEN = '#10b981';

export default function KpiRow({
  spend,
  income,
  limit,
  delta,
}: {
  spend: number;
  income: number;
  limit: number;
  delta: number | null;
}) {
  const net = income - spend;
  const netPositive = net >= 0;

  return (
    <section aria-label="Key metrics" className="flex gap-3 mb-5">
      <KpiTile
        label="Total Spend"
        value={`₹${spend.toLocaleString()}`}
        valueColor={RED}
        delta={delta}
        icon={<ArrowUpRight className="w-4 h-4" style={{ color: RED }} />}
        iconBg="rgba(239, 68, 68, 0.10)"
      />
      <KpiTile
        label="Income"
        value={`₹${income.toLocaleString()}`}
        valueColor={GREEN}
        icon={<ArrowDownRight className="w-4 h-4" style={{ color: GREEN }} />}
        iconBg="rgba(16, 185, 129, 0.10)"
      />
      <KpiTile
        label="Net"
        value={`${netPositive ? '+' : ''}₹${Math.abs(net).toLocaleString()}`}
        valueColor={netPositive ? GREEN : RED}
        icon={
          netPositive ? (
            <ArrowDownRight className="w-4 h-4" style={{ color: GREEN }} />
          ) : (
            <ArrowUpRight className="w-4 h-4" style={{ color: RED }} />
          )
        }
        iconBg={netPositive ? 'rgba(16, 185, 129, 0.10)' : 'rgba(239, 68, 68, 0.10)'}
      />
      {limit > 0 && <LimitBar spent={spend} limit={limit} />}
    </section>
  );
}
