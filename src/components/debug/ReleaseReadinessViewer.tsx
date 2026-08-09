import { useEffect, useState } from 'react';
import { API, DebugMetrics, ReleaseReadinessSnapshot } from '../../lib/ipc';
import { detectRegressions } from './releaseReadinessRegressions';
import GoNoGoPanel from './GoNoGoPanel';
import {
  LocalMetricsPanel,
  QualityGatePanel,
  OutOfRepoPanel,
} from './ReleaseReadinessPanels';

interface ReleaseReadinessViewerProps {
  metrics: DebugMetrics | null;
}

// Minor finding (Doc 43 §5): no view previously existed distinguishing what
// this desktop app can actually verify locally from the Licensing Backend
// admin surface (subscription/billing/tenant analytics), which is a
// separate Vercel + Neon Postgres service with zero code in this repo —
// confirmed independently during the audit. Without this, "release
// readiness" reads as one unified picture when it's really two disjoint
// systems, only one of which this app has any visibility into.
export function ReleaseReadinessViewer({ metrics }: ReleaseReadinessViewerProps) {
  const [snapshots, setSnapshots] = useState<ReleaseReadinessSnapshot[] | null>(null);
  const [capturing, setCapturing] = useState(false);

  const refreshSnapshots = () => {
    API.debug.listReleaseReadinessSnapshots().then(setSnapshots).catch(console.error);
  };

  useEffect(() => {
    refreshSnapshots();
  }, []);

  const captureSnapshot = async () => {
    setCapturing(true);
    try {
      await API.debug.captureReleaseReadinessSnapshot();
      refreshSnapshots();
    } catch (e) {
      console.error(e);
    } finally {
      setCapturing(false);
    }
  };

  // Snapshots are returned newest-first; the latest is the current go/no-go
  // status, compared against the one before it for regression highlighting.
  const latest = snapshots?.[0] ?? null;
  const previous = snapshots?.[1] ?? null;
  const regressions = latest && previous ? detectRegressions(previous.metrics, latest.metrics) : [];

  return (
    <div className="flex flex-col gap-6">
      <GoNoGoPanel
        snapshots={snapshots}
        latest={latest}
        regressions={regressions}
        capturing={capturing}
        onCapture={captureSnapshot}
      />

      <LocalMetricsPanel metrics={metrics} />

      <QualityGatePanel />

      <OutOfRepoPanel />
    </div>
  );
}
