/**
 * Combined release readiness dashboard.
 */
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

/** Combined release readiness dashboard. */
export function ReleaseReadinessViewer({ metrics }: ReleaseReadinessViewerProps) {
  const [snapshots, setSnapshots] = useState<ReleaseReadinessSnapshot[] | null>(null);
  const [capturing, setCapturing] = useState(false);

  /** Reloads stored snapshots. */
  const refreshSnapshots = () => {
    API.debug.listReleaseReadinessSnapshots().then(setSnapshots).catch(console.error);
  };

  useEffect(() => {
    refreshSnapshots();
  }, []);

  /** Captures a new readiness snapshot. */
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
