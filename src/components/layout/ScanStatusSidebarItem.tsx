import { useState, useEffect } from 'react';
import { Loader2, CheckCircle, XCircle, AlertCircle, X } from 'lucide-react';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { useNowTicker } from '@/hooks/useNowTicker';
import { formatDuration } from '@/lib/scanTiming';

export default function ScanStatusSidebarItem() {
  const { scanStatus, scanProgress, scanStartedAt, scanFinishedAt } = useGlobalState();
  const now = useNowTicker(scanStatus === 'running');
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    if (scanStatus === 'running') {
      setDismissed(false);
    }
  }, [scanStatus]);

  if (scanStatus === 'idle' || dismissed) return null;

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

  const showDismiss = scanStatus !== 'running';

  return (
    <div className="px-6 py-2 mb-2 flex flex-col" data-testid="scan-status-sidebar-item">
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-2 mt-1">
          {scanStatus === 'running' && (
            <Loader2 className="w-3.5 h-3.5 animate-spin shrink-0" style={{ color: '#F8E7C9' }} />
          )}
          {scanStatus === 'done' && (
            <CheckCircle className="w-3.5 h-3.5 shrink-0" style={{ color: '#10b981' }} />
          )}
          {scanStatus === 'cancelled' && (
            <XCircle className="w-3.5 h-3.5 shrink-0" style={{ color: 'rgba(248,231,201,0.6)' }} />
          )}
          {scanStatus === 'error' && (
            <AlertCircle className="w-3.5 h-3.5 shrink-0" style={{ color: '#ef4444' }} />
          )}
          <span
            className="text-[13px] font-medium tracking-wide"
            style={{ color: '#F8E7C9' }}
          >
            {label}
          </span>
        </div>
        
        {showDismiss && (
          <button
            onClick={() => setDismissed(true)}
            className="p-1 -mr-2 rounded-md transition-colors"
            style={{ color: 'rgba(248,231,201,0.5)' }}
            aria-label="Dismiss notification"
          >
            <X className="w-4 h-4 hover:text-[#F8E7C9]" />
          </button>
        )}
      </div>

      {scanStatus === 'running' && scanProgress && scanProgress.total > 0 && (
        <div
          className="w-full h-1 rounded-full overflow-hidden mt-3 mb-1"
          style={{ background: 'rgba(248,231,201,0.12)' }}
        >
          <div
            className="h-full rounded-full transition-all duration-300"
            style={{ width: `${progressPct}%`, background: '#F8E7C9' }}
          />
        </div>
      )}

      {scanProgress && (
        <div className="grid grid-cols-2 gap-x-4 gap-y-3 mt-4 mb-2">
          <div className="flex flex-col gap-0.5">
            <span className="text-[10.5px] uppercase tracking-wider font-semibold" style={{ color: 'rgba(248,231,201,0.4)' }}>
              Transactions
            </span>
            <span className="text-[14px] font-medium" style={{ color: '#F8E7C9' }}>
              {scanProgress.transactions_found}
            </span>
          </div>
          <div className="flex flex-col gap-0.5">
            <span className="text-[10.5px] uppercase tracking-wider font-semibold" style={{ color: 'rgba(248,231,201,0.4)' }}>
              Statements
            </span>
            <span className="text-[14px] font-medium" style={{ color: '#F8E7C9' }}>
              {scanProgress.statements_found}
            </span>
          </div>
          <div className="flex flex-col gap-0.5">
            <span className="text-[10.5px] uppercase tracking-wider font-semibold" style={{ color: 'rgba(248,231,201,0.4)' }}>
              Mandates
            </span>
            <span className="text-[14px] font-medium" style={{ color: '#F8E7C9' }}>
              {scanProgress.mandate_events_found}
            </span>
          </div>
          <div className="flex flex-col gap-0.5">
            <span className="text-[10.5px] uppercase tracking-wider font-semibold" style={{ color: 'rgba(248,231,201,0.4)' }}>
              Errors
            </span>
            <span className="text-[14px] font-medium" style={{ color: scanProgress.errors > 0 ? '#ef4444' : '#F8E7C9' }}>
              {scanProgress.errors}
            </span>
          </div>
        </div>
      )}
      
      {elapsedSeconds != null && (
        <div className="text-[11px] font-medium mt-3" style={{ color: 'rgba(248,231,201,0.35)' }}>
          {scanStatus === 'running' ? 'Elapsed' : 'Duration'}: {formatDuration(elapsedSeconds)}
        </div>
      )}
    </div>
  );
}
