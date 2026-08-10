/**
 * Drives the subtle "you have something to review" pulse on the reconciliation
 * navigation item.
 *
 * The problem this solves is timing. A historical scan can create dozens of
 * clusters in quick succession, and pulsing on each one would produce a
 * flickering, ignorable animation. So while a scan is running the events are
 * merely counted, and a single pulse is emitted once the scan finishes -- the
 * user is nudged when they can actually act on the result, not while the work
 * is still in flight.
 *
 * Outside a scan, clusters arrive individually and are pulsed immediately.
 */
import { create } from 'zustand';
import { useSyncStore } from '@/stores/useSyncStore';
import { isTauriRuntime } from '@/lib/tauriRuntime';

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
    // Read the scan store imperatively rather than subscribing -- this runs
    // outside React, and only the value at this instant matters.
    if (useSyncStore.getState().scanStatus === 'running') {
      // Defer: bank the cluster and stay quiet until the scan ends.
      set((s) => ({ pendingSinceLastPulse: s.pendingSinceLastPulse + 1 }));
      return;
    }
    set({ justPulsed: true });
  },

  // One pulse for the whole scan, regardless of how many clusters it produced,
  // and only if it actually produced any. The counter is reset so the next scan
  // starts from a clean slate.
  onScanCompleted: () => {
    if (get().pendingSinceLastPulse > 0) {
      set({ justPulsed: true, pendingSinceLastPulse: 0 });
    }
  },

  // Called by the animation once it has played, so the pulse fires once per
  // trigger instead of latching on.
  clearPulse: () => set({ justPulsed: false }),
}));

// Module-level subscription: established once when this module is first
// imported, and intentionally never torn down, since the store lives for the
// lifetime of the application.
(async () => {
  // No Tauri event bus outside the desktop shell (browser dev, jsdom tests) --
  // the store still works, it simply never receives events.
  if (!isTauriRuntime()) return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen('reconciliation_cluster', () => {
      useReconciliationNudgeStore.getState().onClusterCreated();
    });
    await listen('scan_completed', () => {
      useReconciliationNudgeStore.getState().onScanCompleted();
    });
  } catch (e) {
    // A failed subscription costs the nudge animation, nothing more, so it is
    // logged rather than allowed to break application startup.
    console.error('Failed to subscribe to reconciliation cluster events', e);
  }
})();
