import { formatDuration } from '@/lib/scanTiming';
import type { ScanStatus } from '@/lib/GlobalStateContext';

export default function ScanElapsed({
  scanStatus,
  elapsedSeconds,
  etaSeconds,
}: {
  scanStatus: ScanStatus;
  elapsedSeconds: number;
  etaSeconds: number | null;
}) {
  return (
    <div className="mb-4 text-[12px] text-[#064E3B]/60">
      {scanStatus === 'running'
        ? `Elapsed: ${formatDuration(elapsedSeconds)}`
        : scanStatus === 'done'
          ? `Completed in ${formatDuration(elapsedSeconds)}`
          : `Ran for ${formatDuration(elapsedSeconds)}`}
      {etaSeconds != null && ` · ~${formatDuration(etaSeconds)} remaining`}
    </div>
  );
}
