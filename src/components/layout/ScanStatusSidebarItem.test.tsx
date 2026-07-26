import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import ScanStatusSidebarItem from './ScanStatusSidebarItem';
import { useGlobalState } from '@/lib/GlobalStateContext';

vi.mock('@/lib/GlobalStateContext', () => ({
  useGlobalState: vi.fn(),
}));

const baseProgress = {
  account_id: 'acct_1',
  processed: 10,
  total: 100,
  transactions_found: 2,
  statements_found: 0,
  mandate_events_found: 0,
  non_financial: 3,
  errors: 0,
};

describe('ScanStatusSidebarItem', () => {
  it('renders nothing when idle', () => {
    (useGlobalState as any).mockReturnValue({
      scanStatus: 'idle',
      scanProgress: null,
      scanStartedAt: null,
      scanFinishedAt: null,
    });
    render(<ScanStatusSidebarItem />);
    expect(screen.queryByTestId('scan-status-sidebar-item')).toBeNull();
  });

  it('shows live progress while running', () => {
    (useGlobalState as any).mockReturnValue({
      scanStatus: 'running',
      scanProgress: baseProgress,
      scanStartedAt: Date.now() - 5000,
      scanFinishedAt: null,
    });
    render(<ScanStatusSidebarItem />);
    expect(screen.getByText(/Scanning… 10\/100/)).toBeTruthy();
  });

  it('shows a frozen elapsed time and distinct label once cancelled', () => {
    const startedAt = Date.now() - 12000;
    const finishedAt = Date.now() - 2000;
    (useGlobalState as any).mockReturnValue({
      scanStatus: 'cancelled',
      scanProgress: { ...baseProgress, processed: 5 },
      scanStartedAt: startedAt,
      scanFinishedAt: finishedAt,
    });
    render(<ScanStatusSidebarItem />);
    expect(screen.getByText('Scan cancelled')).toBeTruthy();
    expect(screen.getByText(/10s/)).toBeTruthy();
  });

  it('shows a completed label on success', () => {
    (useGlobalState as any).mockReturnValue({
      scanStatus: 'done',
      scanProgress: { ...baseProgress, processed: 100 },
      scanStartedAt: Date.now() - 30000,
      scanFinishedAt: Date.now(),
    });
    render(<ScanStatusSidebarItem />);
    expect(screen.getByText('Scan complete')).toBeTruthy();
  });

  it('shows a failed label on error', () => {
    (useGlobalState as any).mockReturnValue({
      scanStatus: 'error',
      scanProgress: baseProgress,
      scanStartedAt: Date.now() - 8000,
      scanFinishedAt: Date.now(),
    });
    render(<ScanStatusSidebarItem />);
    expect(screen.getByText('Scan failed')).toBeTruthy();
  });
});
