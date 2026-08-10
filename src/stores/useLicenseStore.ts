/**
 * Holds the current subscription/licensing state for the whole application.
 *
 * Kept in Zustand rather than React Query because this is gating state, not
 * ordinary cached data: `isLocked` decides whether large parts of the UI are
 * reachable at all, so it must be readable synchronously from anywhere,
 * including outside React.
 *
 * Two paths keep it current -- an explicit `hydrate()` at startup, and a
 * long-lived subscription to backend `license_state_changed` events. The second
 * is what lets a subscription purchased or expired mid-session take effect
 * immediately, without the user restarting the app.
 */
import { create } from 'zustand';
import { API, LicenseStatusResponse } from '@/lib/ipc';
import { isTauriRuntime } from '@/lib/tauriRuntime';

interface LicenseStoreState {
  state: string;
  isLocked: boolean;
  daysRemainingInTrial: number | null;
  planId: string | null;
  billingInterval: string | null;
  expiryDate: string | null;
  hydrated: boolean;
  hydrate: () => Promise<void>;
  applyStatus: (status: LicenseStatusResponse) => void;
}

export const useLicenseStore = create<LicenseStoreState>((set) => ({
  // Optimistic defaults: unlocked until the backend says otherwise. Starting
  // locked would flash a paywall during the brief window before hydration
  // completes, which every legitimate user would see on every launch.
  // `hydrated` is what lets the UI distinguish "genuinely unlocked" from
  // "not yet checked".
  state: 'ANONYMOUS_EVAL',
  isLocked: false,
  daysRemainingInTrial: null,
  planId: null,
  billingInterval: null,
  expiryDate: null,
  hydrated: false,

  // Single reducer for both the startup fetch and live events, so a status from
  // either path is projected into state identically. Note `isLocked` inverts
  // the backend's `is_active`.
  applyStatus: (status) =>
    set({
      state: status.state,
      isLocked: !status.is_active,
      daysRemainingInTrial: status.days_remaining,
      planId: status.plan_id,
      billingInterval: status.billing_interval,
      expiryDate: status.expiry_date,
      hydrated: true,
    }),

  /**
   * Fetch the authoritative status from the backend once at startup.
   *
   * On failure `hydrated` deliberately stays false and the app remains
   * unlocked. Locking users out because a status check failed would punish them
   * for a local error; the backend enforces entitlement independently.
   */
  hydrate: async () => {
    try {
      const status = await API.licensing.getStatus();
      useLicenseStore.getState().applyStatus(status);
    } catch (e) {
      console.error('Failed to hydrate license state', e);
    }
  },
}));

// Live updates, subscribed once at module load. The backend pushes a full
// status payload whenever entitlement changes -- a completed purchase, a
// revalidation, an expiry -- so no polling is needed here.
(async () => {
  if (!isTauriRuntime()) return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen<LicenseStatusResponse>('license_state_changed', (event) => {
      useLicenseStore.getState().applyStatus(event.payload);
    });
  } catch (e) {
    console.error('Failed to subscribe to license_state_changed', e);
  }
})();
