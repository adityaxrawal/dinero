import { create } from 'zustand';
import { API, ScanProgressPayload, ScanStatusResponse } from '@/lib/ipc';
import { isTauriRuntime } from '@/lib/tauriRuntime';

interface SystemWarning {
  warning_type: string;
  message: string;
  [key: string]: unknown;
}

/**
 * TASK-FE-002 (Doc 30): mirrors Gmail poll/historical-scan progress and
 * system warnings via `scan_progress`/`scan_completed`/`scan_failed`/
 * `system_warning` events — read-only reflection of backend push state,
 * never a place that itself fetches or caches transaction/instrument rows
 * (that's React Query's job, TASK-FE-003).
 */
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

  onScanProgress: (payload) =>
    set({ scanStatus: 'running', scanProgress: payload, scanError: null }),
  onScanCompleted: () => set({ scanStatus: 'done' }),
  onScanFailed: (message) => set({ scanStatus: 'error', scanError: message }),
  onSystemWarning: (warning) => set((s) => ({ warnings: [...s.warnings, warning] })),
  dismissWarning: (index) => set((s) => ({ warnings: s.warnings.filter((_, i) => i !== index) })),
  resetScanState: () => set({ scanStatus: 'idle', scanProgress: null, scanError: null }),

  /**
   * audit_07 #7: seeds scan state from the persisted checkpoint after a
   * webview reload, when there is no live event to learn it from.
   *
   * Only applies while the store is still untouched (`scanStatus === 'idle'`
   * and no progress yet). Hydration is async, so a live `scan_progress` event
   * can easily land first — and that event is strictly newer than the
   * checkpoint, which is only written every `CHECKPOINT_INTERVAL` messages.
   * Yielding to it is what stops re-hydration from rewinding a running scan's
   * counters.
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
          // Not carried by the checkpoint — the next live event fills it in.
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

  // audit_07 #7: a scan runs in the backend regardless of the webview, so
  // after a reload (Cmd+R, or a webview crash) the UI showed no scan at all
  // until the next `scan_progress` event — which for a slow or nearly-finished
  // scan can be a long time, or never. Subscribe first, then re-hydrate, so a
  // live event is never missed while this is in flight.
  try {
    const accounts = await API.auth.listConnectedAccounts();
    const statuses = await Promise.all(
      accounts.map(async (a) => {
        try {
          return { accountId: a.account_id, status: await API.ingestion.getScanStatus(a.account_id) };
        } catch {
          // One unreadable account must not stop the others from hydrating.
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
