/**
 * Global UI state that outlives any single screen.
 *
 * Zustand rather than React Query because none of this is server data -- it is
 * the user's current view preferences, held in memory only and deliberately not
 * persisted, so each app launch starts from a clean, predictable state.
 *
 * The date range and active instrument act as cross-cutting filters: several
 * screens read them to scope what they display, which is why they live here
 * instead of in any one component's local state.
 */
import { create } from 'zustand';

/** Inclusive filter bounds as ISO date strings; null means unbounded on that end. */
interface DateRangeFilter {
  start: string | null;
  end: string | null;
}

interface AppStoreState {
  dateRange: DateRangeFilter;
  setDateRange: (range: DateRangeFilter) => void;

  activeInstrumentId: string | null;
  setActiveInstrumentId: (id: string | null) => void;

  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
}

export const useAppStore = create<AppStoreState>((set) => ({
  // Unbounded by default: show everything until the user narrows it.
  dateRange: { start: null, end: null },
  setDateRange: (range) => set({ dateRange: range }),

  // null means "all instruments" rather than "none selected".
  activeInstrumentId: null,
  setActiveInstrumentId: (id) => set({ activeInstrumentId: id }),

  sidebarCollapsed: false,
  // Toggle reads through the updater form so it flips whatever the current
  // value is, rather than closing over a value that may already be stale.
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  // Explicit setter for callers that need a definite state, such as collapsing
  // in response to a viewport change rather than a click.
  setSidebarCollapsed: (collapsed) => set({ sidebarCollapsed: collapsed }),
}));
