import { useState, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { API, type ConnectedAccountInfo, type ScanProgressPayload } from '@/lib/ipc';
import { errorMessage } from '@/lib/utils';

export type ScanStatus = 'idle' | 'running' | 'done' | 'error' | 'cancelled';

const CONNECTED_ACCOUNTS_POLL_MS = 3000;

function monthAgo(): string {
  const d = new Date();
  d.setMonth(d.getMonth() - 1);
  return d.toISOString().split('T')[0];
}

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

/** Blocking validation before a scan is worth starting at all. */
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

export function useScanState() {
  const [scanStartDate, setScanStartDate] = useState(monthAgo);
  const [scanEndDate, setScanEndDate] = useState(() => new Date().toISOString().split('T')[0]);
  const [scanStatus, setScanStatus] = useState<ScanStatus>('idle');
  const [scanProgress, setScanProgress] = useState<ScanProgressPayload | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [scanStartedAt, setScanStartedAt] = useState<number | null>(null);
  const [scanFinishedAt, setScanFinishedAt] = useState<number | null>(null);
  const [connectedAccounts, setConnectedAccounts] = useState<ConnectedAccountInfo[]>([]);
  const unlistenScanRef = useRef<(() => void) | null>(null);

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

  // Doc 13 Flow 4.1's scan trigger predates multi-account support (Doc 03
  // §8.2) — scanning always targets the first-connected account; scanning a
  // specific one of several accounts is a Mail Scan UX enhancement outside
  // M02's scope (connecting/gating multiple accounts).
  const primaryConnectedAccount = connectedAccounts[0] ?? null;

  /** Every terminal event stops the progress listener as well as itself. */
  const finish = (apply: () => void) => {
    apply();
    setScanFinishedAt(Date.now());
    if (unlistenScanRef.current) {
      unlistenScanRef.current();
      unlistenScanRef.current = null;
    }
  };

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

  const handleCancelScan = async () => {
    if (!primaryConnectedAccount) return;
    try {
      await API.ingestion.cancelScan(primaryConnectedAccount.account_id);
    } catch (err: unknown) {
      setScanError(errorMessage(err));
      throw err;
    }
  };

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
