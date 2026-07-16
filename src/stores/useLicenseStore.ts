import { create } from 'zustand';
import { API, LicenseStatusResponse } from '@/lib/ipc';

/**
 * TASK-FE-002/016 (Doc 30): mirrors `license_state` reactively so the
 * License Lock Overlay / Grace Period Banner (TASK-FE-016) dismiss
 * immediately on a background revalidation, without a page reload.
 *
 * Backend push side: TASK-API-010 follow-up — `license_activate`/
 * `license_deactivate`/`license_refresh` and the 6-hourly background
 * validation worker all emit `license_state_changed` with a fresh
 * `LicenseStatusResponse` snapshot (src-tauri/src/licensing/commands.rs
 * `emit_license_state_changed`); this store just mirrors that broadcast.
 */
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
  // ANONYMOUS_EVAL is the pre-onboarding default — matches the backend's
  // own default-to-AnonymousEval when no license_state row exists yet.
  state: 'ANONYMOUS_EVAL',
  isLocked: false,
  daysRemainingInTrial: null,
  planId: null,
  billingInterval: null,
  expiryDate: null,
  hydrated: false,

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

  hydrate: async () => {
    try {
      const status = await API.licensing.getStatus();
      useLicenseStore.getState().applyStatus(status);
    } catch (e) {
      console.error('Failed to hydrate license state', e);
    }
  },
}));

// Module-level subscription (once per app lifetime, not per-component) so
// the store starts mirroring `license_state_changed` as soon as it's first
// imported, regardless of which component ends up rendering the overlay.
// Guarded: outside the Tauri runtime (plain browser/vitest), `@tauri-apps/api`
// has nothing to attach to and must not throw during module init.
(async () => {
  const isTauriRuntime =
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  if (!isTauriRuntime) return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen<LicenseStatusResponse>('license_state_changed', (event) => {
      useLicenseStore.getState().applyStatus(event.payload);
    });
  } catch (e) {
    console.error('Failed to subscribe to license_state_changed', e);
  }
})();
