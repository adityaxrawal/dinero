/**
 * Start and cancel controls for a mail scan.
 */
import { Loader2, RotateCcw, ScanLine } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useGlobalState } from '@/lib/GlobalStateContext';
import CancelScanButton from './CancelScanButton';

/** Start and cancel controls for a mail scan. */
export default function ScanControls({ hasAccount }: { hasAccount: boolean }) {
  const {
    scanStatus,
    scanStartDate,
    scanEndDate,
    handleStartScan,
    resetScan,
    failedScanCount,
    handleRetryFailed,
  } = useGlobalState();
  const isFinished =
    scanStatus === 'done' || scanStatus === 'error' || scanStatus === 'cancelled';

  return (
    <div className="flex gap-3">
      <Button
        onClick={handleStartScan}
        disabled={!hasAccount || scanStatus === 'running' || !scanStartDate || !scanEndDate}
        className="h-9 px-4 font-semibold"
        style={{ background: '#064E3B', color: '#F8E7C9' }}
      >
        {scanStatus === 'running' ? (
          <Loader2 className="w-4 h-4 mr-2 animate-spin" />
        ) : (
          <ScanLine className="w-4 h-4 mr-2" />
        )}
        Start Scan
      </Button>

      <CancelScanButton />

      {hasAccount && failedScanCount > 0 && scanStatus !== 'running' && (
        <Button
          variant="outline"
          className="h-9 px-4 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
          onClick={handleRetryFailed}
        >
          <RotateCcw className="w-4 h-4 mr-2" />
          Retry failed ({failedScanCount})
        </Button>
      )}

      {isFinished && (
        <Button
          variant="outline"
          className="h-9 px-4 font-semibold border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
          onClick={resetScan}
        >
          Clear
        </Button>
      )}
    </div>
  );
}
