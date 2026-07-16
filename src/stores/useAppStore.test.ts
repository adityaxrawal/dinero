import { describe, it, expect, beforeEach } from 'vitest';
import { useAppStore } from './useAppStore';

describe('useAppStore', () => {
  beforeEach(() => {
    useAppStore.setState({
      dateRange: { start: null, end: null },
      activeInstrumentId: null,
      sidebarCollapsed: false,
    });
  });

  it('sets the date range filter', () => {
    useAppStore.getState().setDateRange({ start: '2026-01-01', end: '2026-01-31' });
    expect(useAppStore.getState().dateRange).toEqual({ start: '2026-01-01', end: '2026-01-31' });
  });

  it('sets and clears the active instrument filter', () => {
    useAppStore.getState().setActiveInstrumentId('instr-1');
    expect(useAppStore.getState().activeInstrumentId).toBe('instr-1');
    useAppStore.getState().setActiveInstrumentId(null);
    expect(useAppStore.getState().activeInstrumentId).toBeNull();
  });

  it('toggles sidebar collapsed state', () => {
    expect(useAppStore.getState().sidebarCollapsed).toBe(false);
    useAppStore.getState().toggleSidebar();
    expect(useAppStore.getState().sidebarCollapsed).toBe(true);
    useAppStore.getState().toggleSidebar();
    expect(useAppStore.getState().sidebarCollapsed).toBe(false);
  });
});
