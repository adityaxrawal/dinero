import { useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { DateRangePicker } from '@/components/ui/date-picker';
import { Loader2 } from 'lucide-react';
import { useGlobalState } from '@/lib/GlobalStateContext';
import type { ScanProgressPayload } from '@/lib/ipc';
import { scanProgressPercent } from './scanProgressPercent';

/**
 * Final onboarding step: pick a date range and kick off the first Gmail scan.
 *
 * Renders one of three states driven by the shared scan status -- the range
 * picker, live progress, or a completion summary. The scan itself is owned by
 * the global scan state, not by this screen, which is what allows "Continue in
 * Background": the user reaches the dashboard while the scan keeps running.
 */
interface HistoricalScanScreenProps {
  onDone: () => void;
}

// Ceiling on how far back a first scan may reach. Older mail is rarely useful
// and a wider window makes the initial scan disproportionately long.
const MAX_YEARS_BACK = 2;

/** Date as YYYY-MM-DD, the form the backend and the range picker both expect. */
function isoDate(d: Date): string {
  return d.toISOString().split('T')[0];
}

/** Live progress, with the option to leave the scan running and move on. */
function ScanRunning({
  progress,
  onDone,
}: {
  progress: ScanProgressPayload | null;
  onDone: () => void;
}) {
  const pct = scanProgressPercent(progress?.processed ?? 0, progress?.total ?? 0);
  return (
    <div className="space-y-4 text-center animate-in fade-in slide-in-from-bottom-4">
      <Loader2 className="w-8 h-8 mx-auto animate-spin text-primary" aria-hidden="true" />
      <p className="text-sm font-medium">Scanning your Gmail history…</p>
      <div
        className="w-full h-2 bg-secondary rounded-full overflow-hidden"
        role="progressbar"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Historical scan progress"
      >
        <div
          className="h-full bg-[#064E3B] transition-all duration-300"
          style={{ width: `${pct}%` }}
        />
      </div>
      {/* Total renders as an ellipsis until the backend finishes counting the
          mailbox, rather than briefly claiming a total of zero. */}
      <p className="text-xs text-muted-foreground">
        {progress?.processed ?? 0} of {progress?.total ?? '…'} messages processed
      </p>
      <Button
        variant="outline"
        onClick={onDone}
        aria-label="Continue to the dashboard while the scan runs in the background"
      >
        Continue in Background
      </Button>
    </div>
  );
}

/** Completion summary: what the scan actually found, then on to the app. */
function ScanComplete({
  progress,
  onDone,
}: {
  progress: ScanProgressPayload | null;
  onDone: () => void;
}) {
  return (
    <div className="space-y-4 text-center animate-in fade-in slide-in-from-bottom-4">
      <p className="text-sm font-medium">Historical scan complete.</p>
      <p className="text-xs text-muted-foreground">
        Found {progress?.transactions_found ?? 0} transactions and{' '}
        {progress?.statements_found ?? 0} statements.
      </p>
      <Button onClick={onDone} variant="accent" aria-label="Continue to the dashboard">
        Continue
      </Button>
    </div>
  );
}

/** Final onboarding step: pick a range and start the first scan. */
export default function HistoricalScanScreen({ onDone }: HistoricalScanScreenProps) {
  const {
    scanStartDate,
    setScanStartDate,
    scanEndDate,
    setScanEndDate,
    scanStatus,
    scanProgress,
    scanError,
    handleStartScan,
    refreshConnectedAccounts,
  } = useGlobalState();

  // Default to the last three months -- long enough to be useful immediately,
  // short enough that the first scan finishes quickly. Also refreshes the
  // account list, since Gmail was connected on the preceding screen. Runs once
  // on mount; the empty dependency list is intentional, as re-running would
  // overwrite a range the user had already adjusted.
  useEffect(() => {
    const end = new Date();
    const start = new Date();
    start.setMonth(start.getMonth() - 3);
    setScanStartDate(isoDate(start));
    setScanEndDate(isoDate(end));
    refreshConnectedAccounts();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Picker bounds: no further back than the cap, and never into the future.
  const minDate = isoDate(
    new Date(new Date().setFullYear(new Date().getFullYear() - MAX_YEARS_BACK))
  );
  const maxDate = isoDate(new Date());

  if (scanStatus === 'running') {
    return <ScanRunning progress={scanProgress} onDone={onDone} />;
  }

  if (scanStatus === 'done') {
    return <ScanComplete progress={scanProgress} onDone={onDone} />;
  }

  return (
    <div className="space-y-5 animate-in fade-in slide-in-from-bottom-4">
      <div className="p-4 rounded-xl bg-[#F8E7C9]/30 border border-[#064E3B]/10">
        <DateRangePicker
          startDate={scanStartDate}
          endDate={scanEndDate}
          onChange={({ startDate, endDate }) => {
            setScanStartDate(startDate);
            setScanEndDate(endDate);
          }}
          min={minDate}
          max={maxDate}
        />
      </div>

      {scanStatus === 'error' && scanError && (
        <p role="alert" className="text-xs text-red-700">
          {scanError}
        </p>
      )}

      <div className="flex justify-between items-center pt-2">
        <button
          type="button"
          onClick={onDone}
          className="text-sm text-muted-foreground underline underline-offset-2 hover:text-foreground"
        >
          Skip for now
        </button>
        <Button onClick={handleStartScan} variant="accent" aria-label="Start historical scan">
          Start Scan
        </Button>
      </div>
    </div>
  );
}
