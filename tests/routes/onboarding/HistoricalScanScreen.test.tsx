// The onboarding scan screen has three faces — pick a range, watch it run,
// read the result — and each has its own way out.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import HistoricalScanScreen from '@/routes/onboarding/HistoricalScanScreen';
import { useGlobalState } from '@/lib/GlobalStateContext';

vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const handleStartScan = vi.fn();
const refreshConnectedAccounts = vi.fn();
const setScanStartDate = vi.fn();
const setScanEndDate = vi.fn();
const onDone = vi.fn();

function mockState(over: Record<string, unknown> = {}) {
  asMock(useGlobalState).mockImplementation(() => ({
    scanStartDate: '2026-05-01',
    setScanStartDate,
    scanEndDate: '2026-08-01',
    setScanEndDate,
    scanStatus: 'idle',
    scanProgress: null,
    scanError: null,
    handleStartScan,
    refreshConnectedAccounts,
    ...over,
  }));
}

beforeEach(() => {
  vi.clearAllMocks();
  mockState();
});

describe('HistoricalScanScreen, choosing a range', () => {
  it('resets to the onboarding-specific 3-month default on mount', () => {
    render(<HistoricalScanScreen onDone={onDone} />);
    expect(setScanStartDate).toHaveBeenCalled();
    expect(setScanEndDate).toHaveBeenCalled();
  });

  it('refreshes accounts on mount so Start does not race an empty list', () => {
    render(<HistoricalScanScreen onDone={onDone} />);
    expect(refreshConnectedAccounts).toHaveBeenCalled();
  });

  it('starts the scan, and offers a way to skip it entirely', () => {
    render(<HistoricalScanScreen onDone={onDone} />);
    fireEvent.click(screen.getByRole('button', { name: 'Start historical scan' }));
    expect(handleStartScan).toHaveBeenCalled();

    fireEvent.click(screen.getByText('Skip for now'));
    expect(onDone).toHaveBeenCalled();
  });

  it('surfaces a scan error inline', () => {
    mockState({ scanStatus: 'error', scanError: 'Gmail refused the range' });
    render(<HistoricalScanScreen onDone={onDone} />);
    expect(screen.getByRole('alert')).toHaveTextContent('Gmail refused the range');
  });
});

describe('HistoricalScanScreen, mid-scan', () => {
  it('reports progress as a percentage and a count', () => {
    mockState({
      scanStatus: 'running',
      scanProgress: { processed: 25, total: 100 },
    });
    render(<HistoricalScanScreen onDone={onDone} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '25');
    expect(screen.getByText('25 of 100 messages processed')).toBeInTheDocument();
  });

  it('lets the user leave the scan running in the background', () => {
    mockState({ scanStatus: 'running', scanProgress: { processed: 1, total: 10 } });
    render(<HistoricalScanScreen onDone={onDone} />);
    fireEvent.click(screen.getByRole('button', { name: /runs in the background/ }));
    expect(onDone).toHaveBeenCalled();
  });

  it('shows an em dash for a total it does not know yet', () => {
    mockState({ scanStatus: 'running', scanProgress: null });
    render(<HistoricalScanScreen onDone={onDone} />);
    expect(screen.getByText(/0 of … messages processed/)).toBeInTheDocument();
  });
});

describe('HistoricalScanScreen, finished', () => {
  it('reports what the scan found', () => {
    mockState({
      scanStatus: 'done',
      scanProgress: { transactions_found: 42, statements_found: 3 },
    });
    render(<HistoricalScanScreen onDone={onDone} />);
    expect(screen.getByText('Historical scan complete.')).toBeInTheDocument();
    expect(screen.getByText(/42 transactions and 3 statements/)).toBeInTheDocument();
  });

  it('counts zero rather than blank when a scan found nothing', () => {
    mockState({ scanStatus: 'done', scanProgress: null });
    render(<HistoricalScanScreen onDone={onDone} />);
    expect(screen.getByText(/0 transactions and 0 statements/)).toBeInTheDocument();
  });
});
