import { create } from 'zustand';
import { ScanProgressPayload } from '@/lib/ipc';

export interface SystemWarning {
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
}

export const useSyncStore = create<SyncStoreState>((set) => ({
  scanStatus: 'idle',
  scanProgress: null,
  scanError: null,
  warnings: [],

  onScanProgress: (payload) => set({ scanStatus: 'running', scanProgress: payload, scanError: null }),
  onScanCompleted: () => set({ scanStatus: 'done' }),
  onScanFailed: (message) => set({ scanStatus: 'error', scanError: message }),
  onSystemWarning: (warning) => set((s) => ({ warnings: [...s.warnings, warning] })),
  dismissWarning: (index) => set((s) => ({ warnings: s.warnings.filter((_, i) => i !== index) })),
  resetScanState: () => set({ scanStatus: 'idle', scanProgress: null, scanError: null }),
}));

(async () => {
  const isTauriRuntime =
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  if (!isTauriRuntime) return;
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
})();
