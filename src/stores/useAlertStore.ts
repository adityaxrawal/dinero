import { create } from 'zustand';
import { toast } from '@/hooks/use-toast';
import { useSyncStore } from '@/stores/useSyncStore';

/**
 * TASK-RT-003 (Doc 30): mirrors src-tauri's real `AlertPayload`
 * (`reconciliation/alert_worker.rs`) -- `AppLayout.tsx` previously listened
 * for this event with a fabricated `{category, threshold}` shape that never
 * matched what the backend actually emits, and only `console.log`'d it
 * instead of rendering a toast/banner.
 */
export interface AlertThresholdPayload {
  transaction_id: string;
  alert_type: string;
  message: string;
}

const BUDGET_ALERT_PREFIXES = ['global_budget_', 'category_budget_'];

export function isBudgetThresholdAlert(alertType: string): boolean {
  return BUDGET_ALERT_PREFIXES.some((prefix) => alertType.startsWith(prefix));
}

function severityRank(alertType: string): number {
  if (alertType.endsWith('_100')) return 3;
  if (alertType.endsWith('_90')) return 2;
  if (alertType.endsWith('_80')) return 1;
  return 0;
}

export function highestPriorityAlert(
  alerts: AlertThresholdPayload[],
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
 * Doc 30 TASK-RT-003 acceptance `test_bulk_scan_suppresses_individual_transaction_toasts`:
 * "suppressing individual toasts during active bulk-scan mode in favor of
 * the already-visible `scan_progress` counter." Exported as a standalone
 * function (not inlined) so it's directly testable via `useSyncStore`'s own
 * state, and reusable by future toast-triggering call sites (TASK-RT-005,
 * TASK-RT-008) without duplicating the check.
 */
export function shouldSuppressToastDuringBulkScan(): boolean {
  return useSyncStore.getState().scanStatus === 'running';
}

interface AlertStoreState {
  /** One entry per currently-known-crossed `alert_type` this month -- keyed
   * by type so a later, higher-severity crossing (e.g. 80% then 90%)
   * replaces rather than duplicates the earlier one for the same scope. */
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
      alerts: [...s.alerts.filter((a) => a.alert_type !== payload.alert_type), payload],
      // A newly-crossed (necessarily higher) band un-dismisses the banner --
      // same "dismissable-but-recurring" pattern as GracePeriodBanner.
      dismissed: new Set([...s.dismissed].filter((t) => t !== payload.alert_type)),
    }));
  },
  dismissAlert: (alertType) =>
    set((s) => ({ dismissed: new Set(s.dismissed).add(alertType) })),
}));

function toastForAlert(payload: AlertThresholdPayload) {
  const isExhausted = payload.alert_type.endsWith('_100');
  toast({
    title: isExhausted ? 'Budget Exhausted' : 'Spending Alert',
    description: payload.message,
    variant: isExhausted ? 'destructive' : 'default',
  });
}

(async () => {
  const isTauriRuntime =
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  if (!isTauriRuntime) return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
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
