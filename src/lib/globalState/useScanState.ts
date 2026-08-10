/**
 * Owns the lifecycle of a historical Gmail scan.
 *
 * A scan is long-running and entirely backend-driven: the frontend starts it,
 * then learns everything else from four Tauri events -- progress, completed,
 * failed, cancelled. This hook holds the resulting status machine along with the
 * date range that parameterises it, and the list of connected accounts a scan
 * can run against.
 *
 * The three terminal events share one `finish` path, which both records the end
 * time and detaches every listener, so a completed scan stops consuming events
 * and a subsequent scan starts from clean subscriptions.
 */
import { useState, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { API, type ConnectedAccountInfo, type ScanProgressPayload } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';

export type ScanStatus = 'idle' | 'running' | 'done' | 'error' | 'cancelled';

// Accounts are polled rather than pushed, because OAuth connection completes in
// an external browser window that produces no event the app can subscribe to.
const CONNECTED_ACCOUNTS_POLL_MS = 3000;

/** Default scan window start: one month back, as an ISO date string. */
function monthAgo(): string {
  const d = new Date();
  d.setMonth(d.getMonth() - 1);
  return d.toISOString().split('T')[0];
}

// Zeroed progress, set the moment a scan starts so the UI can render a real
// progress panel immediately rather than waiting for the first backend event.
const EMPTY_PROGRESS = (accountId: string): ScanProgressPayload => ({
  account_id: accountId,
  processed: 0,
  total: 0,
  transactions_found: 0,
  statements_found: 0,
  mandate_events_found: 0,
  non_financial: 0,
  errors: 0,
  pending_enrichment: 0,
});

/**
 * Validate the preconditions for starting a scan.
 *
 * Returns the first problem as a message, or null when the request is valid.
 * String comparison is sufficient for the ordering check because ISO dates sort
 * lexicographically.
 */
function scanRangeError(
  account: ConnectedAccountInfo | null,
  start: string,
  end: string
): string | null {
  if (!account) return 'Please connect a Gmail account first.';
  if (!start || !end) return 'Please select both a start and end date.';
  if (start > end) return 'Start date must be before end date.';
  return null;
}

/** Owns the lifecycle of a historical Gmail scan. */
export function useScanState() {
  const [scanStartDate, setScanStartDate] = useState(monthAgo);
  const [scanEndDate, setScanEndDate] = useState(() => new Date().toISOString().split('T')[0]);
  const [scanStatus, setScanStatus] = useState<ScanStatus>('idle');
  const [scanProgress, setScanProgress] = useState<ScanProgressPayload | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [scanStartedAt, setScanStartedAt] = useState<number | null>(null);
  const [scanFinishedAt, setScanFinishedAt] = useState<number | null>(null);
  const [connectedAccounts, setConnectedAccounts] = useState<ConnectedAccountInfo[]>([]);
  // Holds a single composed teardown for all four scan listeners, so they are
  // always attached and detached as one unit.
  const unlistenScanRef = useRef<(() => void) | null>(null);

  /** Reloads the connected-account list. */
  const refreshConnectedAccounts = async () => {
    try {
      setConnectedAccounts(await API.auth.listConnectedAccounts());
    } catch (err) {
      console.error('Failed to fetch connected accounts:', err);
    }
  };

  useEffect(() => {
    refreshConnectedAccounts();
    const interval = setInterval(refreshConnectedAccounts, CONNECTED_ACCOUNTS_POLL_MS);
    return () => clearInterval(interval);
  }, []);

  // Scans currently target one account; the first connected one is used.
  const primaryConnectedAccount = connectedAccounts[0] ?? null;

  /**
   * Shared terminal path for completed, failed and cancelled scans.
   *
   * The caller supplies the status-specific state update; this records the
   * finish time and tears the listeners down, which is what stops a finished
   * scan from reacting to any further events.
   */
  const finish = (apply: () => void) => {
    apply();
    setScanFinishedAt(Date.now());
    if (unlistenScanRef.current) {
      unlistenScanRef.current();
      unlistenScanRef.current = null;
    }
  };

  /**
   * Subscribe to the four scan events and compose one combined teardown.
   *
   * Note the ordering below: the progress unlisten is parked in the ref first,
   * then captured as `prev` and folded into the composed teardown alongside the
   * three terminal listeners, so all four are released together.
   */
  const attachScanListeners = async () => {
    const unlistenProgress = await listen<ScanProgressPayload>('scan_progress', (event) =>
      setScanProgress(event.payload)
    );
    unlistenScanRef.current = unlistenProgress;

    const unlistenDone = await listen<ScanProgressPayload>('scan_completed', (event) =>
      finish(() => {
        setScanStatus('done');
        setScanProgress(event.payload);
      })
    );
    const unlistenError = await listen<{ error: string }>('scan_failed', (event) =>
      finish(() => {
        setScanStatus('error');
        setScanError(event.payload.error);
      })
    );
    const unlistenCancelled = await listen<ScanProgressPayload>('scan_cancelled', (event) =>
      finish(() => {
        setScanStatus('cancelled');
        setScanProgress(event.payload);
      })
    );

    const prev = unlistenScanRef.current;
    unlistenScanRef.current = () => {
      if (prev) prev();
      unlistenDone();
      unlistenError();
      unlistenCancelled();
    };
  };

  /**
   * Validate, reset prior state, subscribe, then ask the backend to begin.
   *
   * State is moved to 'running' before the IPC call so the UI responds
   * immediately; a rejected call rolls it forward to 'error' rather than back to
   * idle, since a failed start is something the user needs told about.
   */
  const handleStartScan = async () => {
    const error = scanRangeError(primaryConnectedAccount, scanStartDate, scanEndDate);
    if (error) {
      alert(error);
      return;
    }

    setScanStatus('running');
    setScanProgress(EMPTY_PROGRESS(primaryConnectedAccount!.account_id));
    setScanError(null);
    setScanStartedAt(Date.now());
    setScanFinishedAt(null);

    try {
      // Release any listeners left over from a previous scan before attaching
      // new ones, so events are never handled twice.
      if (unlistenScanRef.current) unlistenScanRef.current();
      await attachScanListeners();
      await API.ingestion.startHistoricalScan(
        primaryConnectedAccount!.account_id,
        scanStartDate,
        scanEndDate
      );
    } catch (err: unknown) {
      setScanStatus('error');
      setScanError(errorMessage(err));
      setScanFinishedAt(Date.now());
    }
  };

  /**
   * Request cancellation. Status is not changed here -- the backend confirms by
   * emitting `scan_cancelled`, which is what moves the state machine, so the UI
   * never claims a scan stopped before it actually did.
   *
   * The error is re-thrown after being recorded so the calling button can also
   * react to a failed cancellation.
   */
  const handleCancelScan = async () => {
    if (!primaryConnectedAccount) return;
    try {
      await API.ingestion.cancelScan(primaryConnectedAccount.account_id);
    } catch (err: unknown) {
      setScanError(errorMessage(err));
      throw err;
    }
  };

  /** Clear a finished scan back to idle so the panel can be dismissed. */
  const resetScan = () => {
    setScanStatus('idle');
    setScanProgress(null);
    setScanStartedAt(null);
    setScanFinishedAt(null);
  };

  return {
    scanStartDate,
    setScanStartDate,
    scanEndDate,
    setScanEndDate,
    scanStatus,
    setScanStatus,
    scanProgress,
    setScanProgress,
    scanError,
    setScanError,
    scanStartedAt,
    scanFinishedAt,
    connectedAccounts,
    setConnectedAccounts,
    refreshConnectedAccounts,
    handleStartScan,
    handleCancelScan,
    resetScan,
  };
}
