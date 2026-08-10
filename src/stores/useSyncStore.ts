import { create } from 'zustand';
import { API, ScanProgressPayload, ScanStatusResponse } from '@/lib/ipc';
import { isTauriRuntime } from '@/lib/tauriRuntime';

/**
 * Global mirror of backend scan progress, plus the system warning queue.
 *
 * Distinct from the scan state held in GlobalStateContext: that one belongs to
 * the screen that *starts* a scan, whereas this store is a passive,
 * app-wide reflection of whatever the backend is doing, readable synchronously
 * from anywhere -- including from other stores deciding whether to stay quiet
 * during a bulk import.
 *
 * The startup block at the bottom does two things: subscribes to live events,
 * and re-hydrates from the backend. Re-hydration matters because a scan runs in
 * Rust and survives a frontend reload; without it, reopening the window during a
 * long scan would show an idle app while work continued invisibly.
 */
interface SystemWarning {
  warning_type: string;
  message: string;
  [key: string]: unknown;
}

interface SyncStoreState {
  scanStatus: 'idle' | 'running' | 'done' | 'error';
  scanProgress: ScanProgressPayload | null;
  scanError: string | null;
  warnings: SystemWarning[];

  onScanProgress: (payload: ScanProgressPayload) => void;
  onScanCompleted: () => void;
  onScanFailed: (message: string) => void;
  onSystemWarning: (warning: SystemWarning) => void;
  dismissWarning: (index: number) => void;
  resetScanState: () => void;
  hydrateScanState: (accountId: string, status: ScanStatusResponse) => void;
}

export const useSyncStore = create<SyncStoreState>((set) => ({
  scanStatus: 'idle',
  scanProgress: null,
  scanError: null,
  warnings: [],

  // Any progress event implies a running scan and clears a stale error, so a
  // restarted scan does not inherit the previous run's failure message.
  onScanProgress: (payload) =>
    set({ scanStatus: 'running', scanProgress: payload, scanError: null }),
  onScanCompleted: () => set({ scanStatus: 'done' }),
  onScanFailed: (message) => set({ scanStatus: 'error', scanError: message }),
  onSystemWarning: (warning) => set((s) => ({ warnings: [...s.warnings, warning] })),
  // Unlike budget alerts, warning dismissal is persisted backend-side so it
  // survives a restart. The local removal is applied immediately and does not
  // wait on that call -- a failed persist costs a reappearing warning, which is
  // preferable to a UI that stalls on dismiss.
  dismissWarning: (index) =>
    set((s) => {
      const warning = s.warnings[index];
      if (warning && isTauriRuntime()) {
        API.systemWarnings
          .dismiss(warning.warning_type)
          .catch((e) => console.warn('Could not persist warning dismissal', e));
      }
      return { warnings: s.warnings.filter((_, i) => i !== index) };
    }),
  resetScanState: () => set({ scanStatus: 'idle', scanProgress: null, scanError: null }),

  /**
   * Adopt a scan already running in the backend when the frontend starts up.
   *
   * Both guards below make this safe to call late and repeatedly: it refuses to
   * act once any live event has already populated the store, so a slow startup
   * query can never overwrite fresher event data with its own stale snapshot.
   */
  hydrateScanState: (accountId, status) =>
    set((s) => {
      if (s.scanStatus !== 'idle' || s.scanProgress !== null) return s;
      if (status.status !== 'in_progress') return s;
      return {
        scanStatus: 'running',
        scanProgress: {
          account_id: accountId,
          processed: status.processed,
          total: status.total,
          transactions_found: status.transactions_found,
          statements_found: status.statements_found,
          mandate_events_found: status.mandate_events_found,
          // The status query does not report this counter; it fills in from the
          // next live progress event.
          non_financial: 0,
          errors: status.errors,
          pending_enrichment: status.pending_enrichment,
          error_message: null,
        },
        scanError: null,
      };
    }),
}));

(async () => {
  if (!isTauriRuntime()) return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen<ScanProgressPayload>('scan_progress', (event) => {
      useSyncStore.getState().onScanProgress(event.payload);
    });
    await listen('scan_completed', () => {
      useSyncStore.getState().onScanCompleted();
    });
    await listen<{ error_message?: string }>('scan_failed', (event) => {
      useSyncStore.getState().onScanFailed(event.payload?.error_message ?? 'Scan failed');
    });
    await listen<SystemWarning>('system_warning', (event) => {
      useSyncStore.getState().onSystemWarning(event.payload);
    });
  } catch (e) {
    console.error('Failed to subscribe to sync events', e);
  }

  // Re-hydration pass, run after the subscriptions above are in place so that
  // any event arriving meanwhile wins over this snapshot.
  try {
    const accounts = await API.auth.listConnectedAccounts();
    // Queried concurrently, with per-account failures isolated to null so one
    // unreachable account cannot abort hydration for the rest.
    const statuses = await Promise.all(
      accounts.map(async (a) => {
        try {
          return { accountId: a.account_id, status: await API.ingestion.getScanStatus(a.account_id) };
        } catch {
          return null;
        }
      })
    );
    for (const entry of statuses) {
      if (entry) useSyncStore.getState().hydrateScanState(entry.accountId, entry.status);
    }
  } catch (e) {
    console.error('Failed to re-hydrate scan state', e);
  }
})();
