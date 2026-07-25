import { Loader2, CheckCircle, XCircle, AlertCircle } from 'lucide-react';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { useNowTicker } from '@/hooks/useNowTicker';
import { formatDuration } from '@/lib/scanTiming';

export default function ScanStatusSidebarItem() {
  const { scanStatus, scanProgress, scanStartedAt, scanFinishedAt } = useGlobalState();
  const now = useNowTicker(scanStatus === 'running');

  if (scanStatus === 'idle') return null;

  const elapsedSeconds =
    scanStartedAt != null ? ((scanFinishedAt ?? now) - scanStartedAt) / 1000 : null;

  const label =
    scanStatus === 'running'
      ? `Scanning… ${scanProgress ? `${scanProgress.processed}/${scanProgress.total}` : ''}`
      : scanStatus === 'done'
        ? 'Scan complete'
        : scanStatus === 'cancelled'
          ? 'Scan cancelled'
          : 'Scan failed';

  const progressPct =
    scanProgress && scanProgress.total > 0
      ? Math.min(100, (scanProgress.processed / scanProgress.total) * 100)
      : 0;

  return (
    <div className="px-6 mt-3" data-testid="scan-status-sidebar-item">
      <div className="flex items-center gap-2">
        {scanStatus === 'running' && (
          <Loader2 className="w-3 h-3 animate-spin shrink-0" style={{ color: '#F8E7C9' }} />
        )}
        {scanStatus === 'done' && (
          <CheckCircle className="w-3 h-3 shrink-0" style={{ color: '#10b981' }} />
        )}
        {scanStatus === 'cancelled' && (
          <XCircle className="w-3 h-3 shrink-0" style={{ color: 'rgba(248,231,201,0.6)' }} />
        )}
        {scanStatus === 'error' && (
          <AlertCircle className="w-3 h-3 shrink-0" style={{ color: '#ef4444' }} />
        )}
        <span
          className="text-[11px] font-medium truncate"
          style={{ color: 'rgba(248,231,201,0.6)' }}
        >
          {label}
        </span>
      </div>

      {scanStatus === 'running' && scanProgress && scanProgress.total > 0 && (
        <div
          className="mt-1.5 w-full h-1 rounded-full overflow-hidden"
          style={{ background: 'rgba(248,231,201,0.12)' }}
        >
          <div
            className="h-full rounded-full transition-all duration-300"
            style={{ width: `${progressPct}%`, background: '#F8E7C9' }}
          />
        </div>
      )}

      {elapsedSeconds != null && (
        <div className="mt-1 text-[10px]" style={{ color: 'rgba(248,231,201,0.4)' }}>
          {scanStatus === 'running'
            ? `Elapsed ${formatDuration(elapsedSeconds)}`
            : formatDuration(elapsedSeconds)}
        </div>
      )}
    </div>
  );
}
