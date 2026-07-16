import { create } from 'zustand';

export type TransactionDirection = 'debit' | 'credit';

export interface DateRangeFilter {
  start: string | null;
  end: string | null;
}

/**
 * TASK-FE-002 (Doc 30): ephemeral UI-only state — never raw financial data,
 * which is always fetched fresh via React Query (TASK-FE-003) so background
 * ingestion writes can't leave this store holding a stale snapshot.
 */
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
  dateRange: { start: null, end: null },
  setDateRange: (range) => set({ dateRange: range }),

  activeInstrumentId: null,
  setActiveInstrumentId: (id) => set({ activeInstrumentId: id }),

  sidebarCollapsed: false,
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  setSidebarCollapsed: (collapsed) => set({ sidebarCollapsed: collapsed }),
}));
