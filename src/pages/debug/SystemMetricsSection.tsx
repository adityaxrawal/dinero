/**
 * System metrics panel on the debug screen.
 */
import {
  Database,
  FileText,
  ListOrdered,
  Server,
  RefreshCw,
  Activity,
  Settings,
} from 'lucide-react';
import type { DebugMetrics } from '@/lib/ipc';

const TILE =
  'bg-[#F3EBDD]/90 backdrop-blur-md border border-[#064E3B]/10 rounded-2xl p-5 lg:p-6 flex flex-col justify-between gap-4 shadow-[0_2px_10px_rgba(6,78,59,0.03)] hover:shadow-[0_4px_15px_rgba(6,78,59,0.06)] transition-all';
const PANEL =
  'bg-[#F3EBDD]/90 backdrop-blur-md border border-[#064E3B]/10 rounded-2xl p-6 shadow-[0_2px_10px_rgba(6,78,59,0.03)]';

/** One metric with its value. */
function MetricTile({
  label,
  icon,
  iconClassName = 'bg-[#064E3B]/5 text-[#064E3B]/70',
  children,
}: {
  label: string;
  icon: React.ReactNode;
  iconClassName?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={TILE}>
      <div className="flex items-center justify-between gap-2">
        <p className="text-[12px] font-bold uppercase tracking-wider text-[#064E3B]/60 truncate">
          {label}
        </p>
        <div
          className={`w-8 h-8 shrink-0 rounded-full flex items-center justify-center ${iconClassName}`}
        >
          {icon}
        </div>
      </div>
      {children}
    </div>
  );
}

/** A single formatted figure. */
function Figure({ children }: { children: React.ReactNode }) {
  return (
    <div>
      <h3 className="text-3xl lg:text-4xl font-black text-[#064E3B] tracking-tighter truncate">
        {children}
      </h3>
    </div>
  );
}

/**
 * Distribution of values across categories.
 *
 * Used for the extraction-layer breakdown, which shows how much traffic reaches
 * the expensive LLM layer versus being handled by cheap rules.
 */
function DistributionPanel({
  title,
  entries,
}: {
  title: string;
  entries: Record<string, number> | undefined;
}) {
  const rows = Object.entries(entries || {});
  return (
    <div className={PANEL}>
      <h3 className="text-[15px] font-extrabold mb-5 text-[#064E3B] flex items-center gap-2">
        <div className="w-2 h-2 rounded-full bg-[#064E3B]/40" />
        {title}
      </h3>
      <div className="flex flex-col gap-1.5">
        {rows.map(([key, count]) => (
          <div
            key={key}
            className="flex justify-between items-center py-2.5 px-3 rounded-xl hover:bg-[#064E3B]/5 transition-colors border border-transparent hover:border-[#064E3B]/5"
          >
            <span className="text-[13px] font-semibold text-[#064E3B]">{key || 'unspecified'}</span>
            <span className="text-[13px] font-bold text-[#064E3B] bg-[#064E3B]/5 px-3 py-1 rounded-full shadow-sm">
              {count}
            </span>
          </div>
        ))}
        {rows.length === 0 && (
          <div className="py-6 text-center text-[13px] text-[#064E3B]/50 font-medium bg-[#064E3B]/5 rounded-xl border border-dashed border-[#064E3B]/10">
            No data available.
          </div>
        )}
      </div>
    </div>
  );
}

/** Grid layout for the metric tiles. */
function MetricsGrid({ metrics, ram }: { metrics: DebugMetrics; ram: number | null }) {
  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 lg:gap-6">
      <MetricTile label="Transactions" icon={<ListOrdered size={16} />}>
        <Figure>{metrics.total_transactions}</Figure>
      </MetricTile>
      <MetricTile label="Statements" icon={<FileText size={16} />}>
        <Figure>{metrics.total_statements}</Figure>
      </MetricTile>
      <MetricTile
        label="Unresolved"
        icon={<Database size={16} />}
        iconClassName="bg-rose-500/10 text-rose-600"
      >
        <Figure>{metrics.unresolved_clusters}</Figure>
      </MetricTile>
      <MetricTile label="LLM Fallback" icon={<Activity size={16} />}>
        <Figure>{(metrics.llm_fallback_rate * 100).toFixed(1)}%</Figure>
      </MetricTile>
      <MetricTile label="Queue Depth" icon={<Settings size={16} />}>
        <Figure>{metrics.queue_depth}</Figure>
      </MetricTile>
      <MetricTile label="Avail RAM" icon={<Server size={16} />}>
        <div className="flex items-baseline gap-1 truncate">
          <h3 className="text-3xl lg:text-4xl font-black text-[#064E3B] tracking-tighter">
            {ram ? ram : '...'}
          </h3>
          <span className="text-lg font-bold text-[#064E3B]/50">GB</span>
        </div>
      </MetricTile>
    </div>
  );
}

/** System metrics panel. */
export default function SystemMetricsSection({
  metrics,
  ram,
  onRefresh,
}: {
  metrics: DebugMetrics | null;
  ram: number | null;
  onRefresh: () => void;
}) {
  return (
    <div className="animate-in fade-in duration-300 flex flex-col gap-6">
      <div className="flex justify-between items-end mb-2">
        <div>
          <h2 className="text-2xl font-black tracking-tight text-[#064E3B]">System Metrics</h2>
          <p className="text-[13px] font-medium text-[#064E3B]/60 mt-1">
            Overview of the processing pipeline health and memory usage.
          </p>
        </div>
        <button
          className="h-9 px-4 rounded-xl text-[13px] font-bold flex items-center gap-2 bg-[#F3EBDD] border border-[#064E3B]/10 text-[#064E3B] hover:bg-[#064E3B]/5 shadow-sm transition-all active:scale-95"
          onClick={onRefresh}
        >
          <RefreshCw size={14} /> Refresh
        </button>
      </div>

      {metrics ? (
        <>
          <MetricsGrid metrics={metrics} ram={ram} />
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 mt-6">
            <DistributionPanel
              title="Extraction Layer Distribution"
              entries={metrics.extraction_layer_distribution}
            />
            <DistributionPanel
              title="Reconciliation Decisions"
              entries={metrics.reconciliation_decision_distribution}
            />
          </div>
        </>
      ) : (
        <div className="flex items-center justify-center py-12 text-[#064E3B]/60 text-[14px] font-medium animate-pulse">
          Loading metrics...
        </div>
      )}
    </div>
  );
}
