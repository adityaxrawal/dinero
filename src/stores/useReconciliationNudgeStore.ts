import { create } from 'zustand';
import { useSyncStore } from '@/stores/useSyncStore';

/**
 * TASK-RT-006 (Doc 30): "a real-time-incrementing badge count on the
 * Reconciliation nav item, not a disruptive toast per cluster ... burst
 * suppression during an active historical scan defers to the
 * scan-completion summary."
 *
 * The badge's actual *count* is owned by `useReconciliationClusters`
 * (React Query, already auto-invalidated on `reconciliation_cluster` --
 * `useIpcQueryInvalidation.ts`) -- this store owns only the much narrower
 * "should the badge visually pulse right now" concern, kept separate so a
 * burst of clusters arriving mid-scan doesn't produce a rapid-fire strobe
 * of individual pulses: each arrival during an active scan is silently
 * counted instead, and a single aggregate pulse fires once, on
 * `scan_completed`.
 */
interface ReconciliationNudgeState {
  justPulsed: boolean;
  pendingSinceLastPulse: number;
  onClusterCreated: () => void;
  onScanCompleted: () => void;
  clearPulse: () => void;
}

export const useReconciliationNudgeStore = create<ReconciliationNudgeState>((set, get) => ({
  justPulsed: false,
  pendingSinceLastPulse: 0,
  onClusterCreated: () => {
    if (useSyncStore.getState().scanStatus === 'running') {
      set((s) => ({ pendingSinceLastPulse: s.pendingSinceLastPulse + 1 }));
      return;
    }
    set({ justPulsed: true });
  },
  onScanCompleted: () => {
    if (get().pendingSinceLastPulse > 0) {
      set({ justPulsed: true, pendingSinceLastPulse: 0 });
    }
  },
  clearPulse: () => set({ justPulsed: false }),
}));

(async () => {
  const isTauriRuntime =
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  if (!isTauriRuntime) return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen('reconciliation_cluster', () => {
      useReconciliationNudgeStore.getState().onClusterCreated();
    });
    await listen('scan_completed', () => {
      useReconciliationNudgeStore.getState().onScanCompleted();
    });
  } catch (e) {
    console.error('Failed to subscribe to reconciliation cluster events', e);
  }
})();
