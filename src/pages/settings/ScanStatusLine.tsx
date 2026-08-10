/**
 * One-line summary of current scan status.
 */
import { Loader2, XCircle, AlertCircle } from 'lucide-react';
import type { ScanProgressPayload } from '@/lib/ipc';
import type { ScanStatus } from '@/lib/GlobalStateContext';

const STATUS_ICON: Partial<Record<ScanStatus, React.ReactNode>> = {
  running: <Loader2 className="w-4 h-4 animate-spin text-[#064E3B]" />,
  cancelled: <XCircle className="w-4 h-4 text-[#064E3B]/60" />,
  error: <AlertCircle className="w-4 h-4 text-red-600" />,
};

const STATUS_MESSAGE: Partial<Record<ScanStatus, string>> = {
  running: 'Scanning emails…',
  done: 'Scan complete!',
  cancelled: 'Scan cancelled.',
};

/** One-line summary of the current scan status. */
export default function ScanStatusLine({
  scanStatus,
  scanProgress,
}: {
  scanStatus: ScanStatus;
  scanProgress: ScanProgressPayload | null;
}) {
  return (
    <div className="flex items-center gap-2 mb-1 font-semibold text-[14px]">
      {STATUS_ICON[scanStatus]}
      <span className={scanStatus === 'error' ? 'text-red-600' : 'text-[#064E3B]'}>
        {STATUS_MESSAGE[scanStatus] ?? 'Scan failed.'}
        {scanProgress && ` (${scanProgress.processed} / ${scanProgress.total})`}
      </span>
    </div>
  );
}
