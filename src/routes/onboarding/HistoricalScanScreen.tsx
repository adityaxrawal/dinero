import { useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Loader2 } from 'lucide-react';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { scanProgressPercent } from './scanProgressPercent';

interface HistoricalScanScreenProps {
  onDone: () => void;
}

// TASK-GMAIL-007's validation ceiling — a scan range may not reach further
// back than 2 years.
const MAX_YEARS_BACK = 2;

function isoDate(d: Date): string {
  return d.toISOString().split('T')[0];
}

/**
 * TASK-FE-006 (Doc 30): shown after a successful Gmail connection (only
 * `GmailConsentScreen`'s success path reaches this screen — a Statement-Only
 * user who skips Gmail entirely has no account to scan and never sees it).
 * Reuses `GlobalStateContext`'s existing scan state/`handleStartScan` (the
 * same machinery Settings' manual "Sync Now" already drives) rather than
 * duplicating the `scan_progress`/`scan_completed`/`scan_failed` listener
 * wiring a second time.
 */
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

  useEffect(() => {
    // Doc30's default (last 3 months) differs from GlobalStateContext's own
    // default (last 1 month, tuned for the Settings manual-sync use case) —
    // reset to the onboarding-specific default every time this screen mounts.
    const end = new Date();
    const start = new Date();
    start.setMonth(start.getMonth() - 3);
    setScanStartDate(isoDate(start));
    setScanEndDate(isoDate(end));
    // The Gmail account just connected in the previous step; refresh now
    // rather than waiting for GlobalStateContext's 3s poll, so handleStartScan
    // doesn't race an empty connectedAccounts list.
    refreshConnectedAccounts();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const minDate = isoDate(
    new Date(new Date().setFullYear(new Date().getFullYear() - MAX_YEARS_BACK))
  );
  const maxDate = isoDate(new Date());

  if (scanStatus === 'running') {
    const pct = scanProgressPercent(scanProgress?.processed ?? 0, scanProgress?.total ?? 0);
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
        <p className="text-xs text-muted-foreground">
          {scanProgress?.processed ?? 0} of {scanProgress?.total ?? '…'} messages processed
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

  if (scanStatus === 'done') {
    return (
      <div className="space-y-4 text-center animate-in fade-in slide-in-from-bottom-4">
        <p className="text-sm font-medium">Historical scan complete.</p>
        <p className="text-xs text-muted-foreground">
          Found {scanProgress?.transactions_found ?? 0} transactions and{' '}
          {scanProgress?.statements_found ?? 0} statements.
        </p>
        <Button onClick={onDone} variant="accent" aria-label="Continue to the dashboard">
          Continue
        </Button>
      </div>
    );
  }

  return (
    <div className="space-y-4 animate-in fade-in slide-in-from-bottom-4">
      <div className="space-y-2">
        <Label htmlFor="scan-start">Scan From</Label>
        <Input
          id="scan-start"
          type="date"
          min={minDate}
          max={scanEndDate || maxDate}
          value={scanStartDate}
          onChange={(e) => setScanStartDate(e.target.value)}
        />
      </div>
      <div className="space-y-2">
        <Label htmlFor="scan-end">Scan To</Label>
        <Input
          id="scan-end"
          type="date"
          min={scanStartDate || minDate}
          max={maxDate}
          value={scanEndDate}
          onChange={(e) => setScanEndDate(e.target.value)}
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
