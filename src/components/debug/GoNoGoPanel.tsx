/**
 * Release go/no-go status from the acceptance-criteria gate.
 */
import { ShieldCheck, CheckCircle2, XCircle, TrendingUp } from 'lucide-react';
import type { ReleaseReadinessSnapshot } from '@/lib/ipc';

/** One readiness snapshot row. */
function SnapshotRow({ snapshot }: { snapshot: ReleaseReadinessSnapshot }) {
  return (
    <div className="flex justify-between items-center text-xs py-1 border-b border-[var(--border-color)] last:border-0">
      <span>{new Date(snapshot.captured_at).toLocaleString()}</span>
      <span className="font-mono">
        clusters={snapshot.metrics.unresolved_clusters} llm=
        {(snapshot.metrics.llm_fallback_rate * 100).toFixed(1)}%
      </span>
      <span className={snapshot.go_no_go ? 'text-emerald-500' : 'text-red-500'}>
        {snapshot.go_no_go ? 'GO' : 'NO-GO'}
      </span>
    </div>
  );
}

/** The most recent go/no-go verdict. */
function LatestVerdict({ latest }: { latest: ReleaseReadinessSnapshot | null }) {
  if (!latest) {
    return (
      <p className="text-sm text-muted-foreground mt-2">
        No snapshot yet. Reflects the most recent
        <code className="text-xs bg-muted px-1 rounded mx-1">
          verify_acceptance_criteria.py --output release_readiness_check.json
        </code>
        run, if any — never invoked by this app itself.
      </p>
    );
  }
  return (
    <div className="flex items-center gap-2 mt-2">
      {latest.go_no_go ? (
        <CheckCircle2 size={16} className="text-emerald-500" />
      ) : (
        <XCircle size={16} className="text-red-500" />
      )}
      <span className="text-sm">
        {latest.go_no_go ? 'GO' : 'NO-GO'} — last captured{' '}
        {new Date(latest.captured_at).toLocaleString()}
      </span>
    </div>
  );
}

/** Release go/no-go status from the acceptance-criteria gate. */
export default function GoNoGoPanel({
  snapshots,
  latest,
  regressions,
  capturing,
  onCapture,
}: {
  snapshots: ReleaseReadinessSnapshot[] | null;
  latest: ReleaseReadinessSnapshot | null;
  regressions: string[];
  capturing: boolean;
  onCapture: () => void;
}) {
  return (
    <div className="glass-panel p-6">
      <div className="flex items-center justify-between mb-1">
        <div className="flex items-center gap-2">
          <ShieldCheck
            size={18}
            className={latest?.go_no_go ? 'text-emerald-500' : 'text-amber-500'}
          />
          <h3 className="font-medium">Release Go / No-Go</h3>
        </div>
        <button
          type="button"
          onClick={onCapture}
          disabled={capturing}
          className="text-xs px-3 py-1.5 rounded-md border border-[var(--border-color)] hover:bg-muted disabled:opacity-50"
        >
          {capturing ? 'Capturing…' : 'Capture Snapshot'}
        </button>
      </div>

      <LatestVerdict latest={latest} />

      {snapshots && snapshots.length > 0 && (
        <div className="mt-4 flex flex-col gap-1">
          <div className="flex items-center gap-2 mb-1">
            <TrendingUp size={14} className="text-muted-foreground" />
            <span className="text-xs text-muted-foreground">Recent snapshots (newest first)</span>
          </div>
          {snapshots.slice(0, 10).map((s) => (
            <SnapshotRow key={s.id} snapshot={s} />
          ))}
          {regressions.length > 0 && (
            <p className="text-xs text-red-500 mt-2">
              Regressed vs. previous snapshot: {regressions.join(', ')}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
