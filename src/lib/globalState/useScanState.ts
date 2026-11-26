/**
 * Owns the lifecycle of a historical Gmail scan.
 *
 * A scan is long-running and entirely backend-driven: the frontend starts it,
 * then learns everything else from four Tauri events -- progress, completed,
 * failed, cancelled. This hook holds the resulting status machine along with the
 * date range that parameterises it, and the list of connected accounts a scan
 * can run against.
 *
 * The listeners stay attached while the account is shown, so scans the backend
 * starts on its own, and scans already running when the window opens, are
 * followed too (review P8).
 */
import { useState, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { API, type ConnectedAccountInfo, type ScanProgressPayload } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';
import { toast } from '@/hooks/use-toast';
import { getErrorToast } from '@/lib/errorMapping';

export type ScanStatus = 'idle' | 'running' | 'done' | 'error' | 'cancelled';

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
  sent_to_review: 0,
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
  // Opt-in: re-evaluate emails earlier scans rejected (cleared after use).
  const [recheckRejected, setRecheckRejected] = useState(false);
  const [scanProgress, setScanProgress] = useState<ScanProgressPayload | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [scanStartedAt, setScanStartedAt] = useState<number | null>(null);
  const [scanFinishedAt, setScanFinishedAt] = useState<number | null>(null);
  const [connectedAccounts, setConnectedAccounts] = useState<ConnectedAccountInfo[]>([]);

  /** Reloads the connected-account list. */
  const refreshConnectedAccounts = async () => {
    try {
      setConnectedAccounts(await API.auth.listConnectedAccounts());
    } catch (err) {
      console.error('Failed to fetch connected accounts:', err);
      toast({ variant: 'destructive', ...getErrorToast(err, 'Could not load your connected accounts.') });
    }
  };

  // Refreshed when the backend reports a change (connect, disconnect, status)
  // and when the window regains focus, instead of polling every 3 seconds for
  // the whole session (review M9).
  useEffect(() => {
    refreshConnectedAccounts();
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen('connected_accounts_changed', () => refreshConnectedAccounts()).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    const onFocus = () => refreshConnectedAccounts();
    window.addEventListener('focus', onFocus);
    return () => {
      cancelled = true;
      unlisten?.();
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  // Scans currently target one account; the first connected one is used.
  const primaryConnectedAccount = connectedAccounts[0] ?? null;

  // Messages that failed in earlier scans, offered for retry. Re-read when the
  // account changes and whenever a scan stops running.
  const [failedScanCount, setFailedScanCount] = useState(0);
  const primaryAccountId = primaryConnectedAccount?.account_id;
  useEffect(() => {
    if (!primaryAccountId || scanStatus === 'running') return;
    API.ingestion
      .getFailedScanCount(primaryAccountId)
      .then(setFailedScanCount)
      .catch(() => setFailedScanCount(0));
  }, [primaryAccountId, scanStatus]);

  // Mirrors scanStatus for the event handlers below, which outlive renders.
  const scanStatusRef = useRef<ScanStatus>('idle');
  useEffect(() => {
    scanStatusRef.current = scanStatus;
  }, [scanStatus]);

  /** Shared terminal path for completed, failed and cancelled scans. */
  const finish = (apply: () => void) => {
    apply();
    setScanFinishedAt(Date.now());
  };

  /**
   * Follow the primary account's scan for as long as that account is shown,
   * whoever started it (review P8). The backend also starts scans itself
   * (catch-up after a history reset, deferred catch-up), and a scan survives a
   * frontend reload, so the listeners cannot depend on the Start button, and
   * the state is first adopted from `scans_status`. A stale `in_progress`
   * checkpoint is reset at boot, so `in_progress` means a scan is running.
   */
  useEffect(() => {
    if (!primaryAccountId) return;
    let cancelled = false;
    const unlistens: Array<() => void> = [];
    const mine = (p: { account_id: string }) => p.account_id === primaryAccountId;

    const adoptRunning = (progress: ScanProgressPayload) => {
      if (scanStatusRef.current !== 'running') {
        setScanStatus('running');
        setScanError(null);
        setScanStartedAt(Date.now());
        setScanFinishedAt(null);
      }
      setScanProgress(progress);
    };

    const subscribe = async () => {
      const handlers = [
        listen<ScanProgressPayload>('scan_progress', (e) => {
          if (mine(e.payload)) adoptRunning(e.payload);
        }),
        listen<ScanProgressPayload>('scan_completed', (e) => {
          if (mine(e.payload))
            finish(() => {
              setScanStatus('done');
              setScanProgress(e.payload);
            });
        }),
        listen<ScanProgressPayload>('scan_failed', (e) => {
          if (mine(e.payload))
            finish(() => {
              setScanStatus('error');
              setScanError(e.payload.error_message ?? 'Scan failed');
            });
        }),
        listen<ScanProgressPayload>('scan_cancelled', (e) => {
          if (mine(e.payload))
            finish(() => {
              setScanStatus('cancelled');
              setScanProgress(e.payload);
            });
        }),
      ];
      for (const h of handlers) {
        const fn = await h;
        if (cancelled) fn();
        else unlistens.push(fn);
      }
      if (cancelled) return;
      // Hydrate after subscribing, so a live event is never missed; a live
      // event that already arrived wins over this snapshot.
      try {
        const status = await API.ingestion.getScanStatus(primaryAccountId);
        if (cancelled || status.status !== 'in_progress' || scanStatusRef.current === 'running')
          return;
        adoptRunning({
          account_id: primaryAccountId,
          processed: status.processed,
          total: status.total,
          transactions_found: status.transactions_found,
          statements_found: status.statements_found,
          mandate_events_found: status.mandate_events_found,
          non_financial: 0,
          errors: status.errors,
          pending_enrichment: status.pending_enrichment,
          sent_to_review: status.sent_to_review,
          error_message: null,
        });
      } catch (err) {
        console.error('Failed to read scan status:', err);
      }
    };
    subscribe();
    return () => {
      cancelled = true;
      for (const fn of unlistens) fn();
    };
  }, [primaryAccountId]);

  /**
   * Validate, reset prior state, then ask the backend to begin.
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
    await beginScan(() =>
      API.ingestion.startHistoricalScan(
        primaryConnectedAccount!.account_id,
        scanStartDate,
        scanEndDate,
        recheckRejected
      )
    );
  };

  /** Re-run only the messages that failed in earlier scans. */
  const handleRetryFailed = async () => {
    if (!primaryConnectedAccount) return;
    await beginScan(() => API.ingestion.retryFailedScan(primaryConnectedAccount.account_id));
  };

  /** Shared start path: reset state, then invoke `start`. */
  const beginScan = async (start: () => Promise<string>) => {
    scanStatusRef.current = 'running';
    setScanStatus('running');
    setScanProgress(EMPTY_PROGRESS(primaryConnectedAccount!.account_id));
    setScanError(null);
    setScanStartedAt(Date.now());
    setScanFinishedAt(null);

    try {
      await start();
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
    failedScanCount,
    handleRetryFailed,
    recheckRejected,
    setRecheckRejected,
  };
}
