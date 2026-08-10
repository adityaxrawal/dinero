/**
 * Individual panels making up the release readiness view.
 */
import { Gauge, ShieldCheck, ExternalLink } from 'lucide-react';
import type { DebugMetrics } from '@/lib/ipc';
import { QUALITY_GATE_TARGETS } from './qualityGateTargets';

/** Locally computed readiness metrics. */
export function LocalMetricsPanel({ metrics }: { metrics: DebugMetrics | null }) {
  const tiles = metrics
    ? [
        { label: 'Transactions', value: metrics.total_transactions },
        { label: 'Statements', value: metrics.total_statements },
        { label: 'Unresolved Clusters', value: metrics.unresolved_clusters },
        { label: 'LLM Fallback Rate', value: `${(metrics.llm_fallback_rate * 100).toFixed(1)}%` },
      ]
    : [];

  return (
    <div className="glass-panel p-6">
      <div className="flex items-center gap-2 mb-1">
        <Gauge size={18} className="text-accent" />
        <h3 className="font-medium">Locally-Verifiable Metrics</h3>
      </div>
      <p className="text-sm text-muted-foreground mb-4">
        Measured directly from this device&apos;s encrypted local database — no server round-trip.
      </p>
      {metrics ? (
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
          {tiles.map((t) => (
            <div key={t.label}>
              <p className="text-xs text-muted-foreground">{t.label}</p>
              <p className="text-lg font-semibold">{t.value}</p>
            </div>
          ))}
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">Loading...</p>
      )}
    </div>
  );
}

/** Metrics judged against their quality-gate targets. */
export function QualityGatePanel() {
  return (
    <div className="glass-panel p-6">
      <div className="flex items-center gap-2 mb-1">
        <ShieldCheck size={18} className="text-accent" />
        <h3 className="font-medium">Quality Gate Targets</h3>
      </div>
      <p className="text-sm text-muted-foreground mb-4">
        Thresholds this app&apos;s test suite checks against — declared targets, not a live
        production measurement pipeline.
      </p>
      <div className="flex flex-col gap-2">
        {QUALITY_GATE_TARGETS.map((g) => (
          <div
            key={g.nfr}
            className="flex justify-between items-center py-2 border-b border-[var(--border-color)] last:border-0"
          >
            <span className="text-sm">
              {g.label} <span className="text-xs text-muted-foreground">({g.nfr})</span>
            </span>
            <span className="text-sm font-mono">{g.target}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Readiness signals that originate outside this repository. */
export function OutOfRepoPanel() {
  return (
    <div className="glass-panel p-6 border border-amber-500/30">
      <div className="flex items-center gap-2 mb-1">
        <ExternalLink size={18} className="text-amber-500" />
        <h3 className="font-medium">Out of Repo: Licensing Backend</h3>
      </div>
      <p className="text-sm text-muted-foreground">
        Subscription analytics, billing operations, and tenant administration are handled by a
        separate Licensing Backend service (Vercel + Neon Postgres) that is{' '}
        <strong>not part of this repository</strong> and has no code, UI, or data visible inside
        this desktop app. This app&apos;s only connection to it is the client module{' '}
        <code className="text-xs bg-muted px-1 rounded">src-tauri/src/licensing/</code>, used
        solely to validate this device&apos;s own license state — not to administer other tenants.
      </p>
    </div>
  );
}
