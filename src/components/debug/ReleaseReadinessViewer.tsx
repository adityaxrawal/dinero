import { ShieldCheck, ExternalLink, Gauge } from 'lucide-react';
import { DebugMetrics } from '../../lib/ipc';

interface ReleaseReadinessViewerProps {
  metrics: DebugMetrics | null;
}

// Doc 43 §5 fix: this app's own quality-gate documentation (phase10's
// test_quality_gates_thresholds) never claims these are *measured in
// production* — they're target thresholds the test suite checks against
// fixed literals, not values derived from real usage data. Presented here as
// declared targets, not live metrics, to avoid the same "fabricated live
// number" pattern already fixed elsewhere in this app (G9-G13).
const QUALITY_GATE_TARGETS = [
  { label: 'Extraction Accuracy', target: '≥ 95%', nfr: 'NFR-003' },
  { label: 'False Positive Rate', target: '< 0.1%', nfr: 'NFR-004' },
  { label: 'False Merge Rate', target: '< 0.1%', nfr: 'NFR-005' },
];

// Minor finding (Doc 43 §5): no view previously existed distinguishing what
// this desktop app can actually verify locally from the Licensing Backend
// admin surface (subscription/billing/tenant analytics), which is a
// separate Vercel + Neon Postgres service with zero code in this repo —
// confirmed independently during the audit. Without this, "release
// readiness" reads as one unified picture when it's really two disjoint
// systems, only one of which this app has any visibility into.
export function ReleaseReadinessViewer({ metrics }: ReleaseReadinessViewerProps) {
  return (
    <div className="flex flex-col gap-6">
      <div className="glass-panel p-6">
        <div className="flex items-center gap-2 mb-1">
          <Gauge size={18} className="text-accent" />
          <h3 className="font-medium">Locally-Verifiable Metrics</h3>
        </div>
        <p className="text-sm text-muted-foreground mb-4">
          Measured directly from this device's encrypted local database — no server round-trip.
        </p>
        {metrics ? (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
            <div>
              <p className="text-xs text-muted-foreground">Transactions</p>
              <p className="text-lg font-semibold">{metrics.total_transactions}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Statements</p>
              <p className="text-lg font-semibold">{metrics.total_statements}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Unresolved Clusters</p>
              <p className="text-lg font-semibold">{metrics.unresolved_clusters}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">LLM Fallback Rate</p>
              <p className="text-lg font-semibold">{(metrics.llm_fallback_rate * 100).toFixed(1)}%</p>
            </div>
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">Loading...</p>
        )}
      </div>

      <div className="glass-panel p-6">
        <div className="flex items-center gap-2 mb-1">
          <ShieldCheck size={18} className="text-accent" />
          <h3 className="font-medium">Quality Gate Targets</h3>
        </div>
        <p className="text-sm text-muted-foreground mb-4">
          Thresholds this app's test suite checks against — declared targets, not a live production
          measurement pipeline.
        </p>
        <div className="flex flex-col gap-2">
          {QUALITY_GATE_TARGETS.map((g) => (
            <div key={g.nfr} className="flex justify-between items-center py-2 border-b border-[var(--border-color)] last:border-0">
              <span className="text-sm">{g.label} <span className="text-xs text-muted-foreground">({g.nfr})</span></span>
              <span className="text-sm font-mono">{g.target}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="glass-panel p-6 border border-amber-500/30">
        <div className="flex items-center gap-2 mb-1">
          <ExternalLink size={18} className="text-amber-500" />
          <h3 className="font-medium">Out of Repo: Licensing Backend</h3>
        </div>
        <p className="text-sm text-muted-foreground">
          Subscription analytics, billing operations, and tenant administration are handled by a
          separate Licensing Backend service (Vercel + Neon Postgres) that is <strong>not part of this
          repository</strong> and has no code, UI, or data visible inside this desktop app. This app's
          only connection to it is the client module <code className="text-xs bg-muted px-1 rounded">src-tauri/src/licensing/</code>,
          used solely to validate this device's own license state — not to administer other tenants.
        </p>
      </div>
    </div>
  );
}
