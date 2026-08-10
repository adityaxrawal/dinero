/**
 * Live progress panel for a running scan.
 */
import { cn } from '@/lib/utils';
import { estimateEtaSeconds } from '@/lib/scanTiming';
import { useNowTicker } from '@/hooks/useNowTicker';
import type { ScanProgressPayload } from '@/lib/ipc';
import type { ScanStatus } from '@/lib/GlobalStateContext';
import ScanStatusLine from './ScanStatusLine';
import ScanElapsed from './ScanElapsed';
import ScanStatsGrid from './ScanStatsGrid';

const PANEL_TONE: Partial<Record<ScanStatus, string>> = {
  error: 'bg-red-500/10 border-red-500/20',
  done: 'bg-emerald-500/10 border-emerald-500/20',
};

interface ScanProgressPanelProps {
  scanStatus: ScanStatus;
  scanProgress: ScanProgressPayload | null;
  scanStartedAt: number | null;
  scanFinishedAt: number | null;
  scanError: string | null;
}

/** Live progress panel for a running scan. */
export default function ScanProgressPanel({
  scanStatus,
  scanProgress,
  scanStartedAt,
  scanFinishedAt,
  scanError,
}: ScanProgressPanelProps) {
  const now = useNowTicker(scanStatus === 'running');
  const elapsedSeconds =
    scanStartedAt != null ? ((scanFinishedAt ?? now) - scanStartedAt) / 1000 : null;
  const etaSeconds =
    scanStatus === 'running' && scanProgress
      ? estimateEtaSeconds(scanProgress.processed, scanProgress.total, elapsedSeconds ?? 0)
      : null;
  const percent =
    scanProgress && scanProgress.total > 0 ? (scanProgress.processed / scanProgress.total) * 100 : 0;

  return (
    <div
      className={cn('p-5 rounded-xl border', PANEL_TONE[scanStatus] ?? 'bg-[#064E3B]/5 border-[#064E3B]/10')}
    >
      <ScanStatusLine scanStatus={scanStatus} scanProgress={scanProgress} />

      {elapsedSeconds != null && (
        <ScanElapsed
          scanStatus={scanStatus}
          elapsedSeconds={elapsedSeconds}
          etaSeconds={etaSeconds}
        />
      )}

      {scanProgress && scanStatus === 'running' && (
        <div className="w-full h-1.5 rounded-full overflow-hidden bg-[#064E3B]/10 mb-5">
          <div
            className="h-full bg-[#064E3B] transition-all duration-300"
            style={{ width: `${percent}%` }}
          />
        </div>
      )}

      {scanProgress && <ScanStatsGrid progress={scanProgress} />}

      {scanError && (
        <div className="mt-3 text-sm text-red-600 bg-red-50 p-2 rounded border border-red-200">
          {scanError}
        </div>
      )}
    </div>
  );
}
