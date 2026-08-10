import { create } from 'zustand';
import { toast } from '@/hooks/use-toast';
import { useSyncStore } from '@/stores/useSyncStore';
import { isTauriRuntime } from '@/lib/tauriRuntime';

/**
 * Budget threshold alerts: which are live, which the user has dismissed.
 *
 * The backend emits an alert each time spending crosses a budget boundary at
 * 80%, 90% or 100%. Two rules keep that from becoming noise. Within a budget,
 * only the most recent alert is retained -- crossing 90% supersedes the earlier
 * 80% rather than stacking beside it. And no toast is raised at all while a bulk
 * scan is running, since importing months of history legitimately trips every
 * threshold at once and would bury the user in notifications about the past.
 *
 * Dismissal is in-memory only, so an alert reappears on the next launch while
 * the underlying budget is still exceeded.
 */
export interface AlertThresholdPayload {
  transaction_id: string;
  alert_type: string;
  message: string;
}

// Alert-type prefixes this store handles. Other alert kinds share the same
// backend event and are filtered out rather than stored here.
const BUDGET_ALERT_PREFIXES = ['global_budget_', 'category_budget_'];

/** Whether an alert type is a budget threshold this store should track. */
export function isBudgetThresholdAlert(alertType: string): boolean {
  return BUDGET_ALERT_PREFIXES.some((prefix) => alertType.startsWith(prefix));
}

/**
 * Order alerts by how serious they are, read from the type's numeric suffix.
 *
 * Unrecognised suffixes rank 0, so an unknown alert never outranks a real
 * threshold crossing when picking the most important one to display.
 */
function severityRank(alertType: string): number {
  if (alertType.endsWith('_100')) return 3;
  if (alertType.endsWith('_90')) return 2;
  if (alertType.endsWith('_80')) return 1;
  return 0;
}

/**
 * Pick the single most severe alert from a list.
 *
 * Used where there is room to show only one banner. Strictly greater-than means
 * ties resolve to the earliest, keeping the choice stable across re-renders.
 */
export function highestPriorityAlert(
  alerts: AlertThresholdPayload[]
): AlertThresholdPayload | null {
  let best: AlertThresholdPayload | null = null;
  for (const a of alerts) {
    if (!best || severityRank(a.alert_type) > severityRank(best.alert_type)) {
      best = a;
    }
  }
  return best;
}

/**
 * Whether toasts should be withheld right now.
 *
 * True during a bulk scan: importing historical data crosses thresholds that
 * were crossed months ago, and toasting each one would be both alarming and
 * useless. The alerts are still recorded in the store, so the banner reflects
 * them once the scan ends.
 */
export function shouldSuppressToastDuringBulkScan(): boolean {
  return useSyncStore.getState().scanStatus === 'running';
}

interface AlertStoreState {
  alerts: AlertThresholdPayload[];
  dismissed: Set<string>;
  onAlertThresholdCrossed: (payload: AlertThresholdPayload) => void;
  dismissAlert: (alertType: string) => void;
}

export const useAlertStore = create<AlertStoreState>((set) => ({
  alerts: [],
  dismissed: new Set(),
  onAlertThresholdCrossed: (payload) => {
    if (!isBudgetThresholdAlert(payload.alert_type)) return;
    set((s) => ({
      // Replace rather than append: filtering out the same alert_type first
      // means one entry per threshold, with the newest payload winning.
      alerts: [...s.alerts.filter((a) => a.alert_type !== payload.alert_type), payload],
      // Re-crossing clears a previous dismissal, so a fresh breach is shown
      // again rather than staying permanently hidden by an earlier dismiss.
      dismissed: new Set([...s.dismissed].filter((t) => t !== payload.alert_type)),
    }));
  },
  // A new Set is constructed rather than mutated, so Zustand sees a changed
  // reference and subscribers actually re-render.
  dismissAlert: (alertType) => set((s) => ({ dismissed: new Set(s.dismissed).add(alertType) })),
}));

/** Raise a toast for one alert, escalating tone at full budget exhaustion. */
function toastForAlert(payload: AlertThresholdPayload) {
  const isExhausted = payload.alert_type.endsWith('_100');
  toast({
    title: isExhausted ? 'Budget Exhausted' : 'Spending Alert',
    description: payload.message,
    variant: isExhausted ? 'destructive' : 'default',
  });
}

(async () => {
  if (!isTauriRuntime()) return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    // Recording and toasting are deliberately separate steps: the alert is
    // always stored, but the toast is conditional on not being mid-scan.
    await listen<AlertThresholdPayload>('alert_threshold_crossed', (event) => {
      useAlertStore.getState().onAlertThresholdCrossed(event.payload);
      if (!shouldSuppressToastDuringBulkScan()) {
        toastForAlert(event.payload);
      }
    });
  } catch (e) {
    console.error('Failed to subscribe to alert_threshold_crossed', e);
  }
})();
