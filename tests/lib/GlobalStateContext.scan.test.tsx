import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import { GlobalStateProvider, useGlobalState } from '@/lib/GlobalStateContext';

const cancelScanMock = vi.fn().mockResolvedValue('cancel_requested');
const startHistoricalScanMock = vi.fn().mockResolvedValue('started');
const getScanStatusMock = vi.fn();
const NOT_STARTED = {
  status: 'not_started',
  processed: 0,
  total: 0,
  transactions_found: 0,
  statements_found: 0,
  mandate_events_found: 0,
  errors: 0,
  pending_enrichment: 0,
};

vi.mock('@/lib/ipc', () => ({
  API: {
    auth: {
      listConnectedAccounts: vi
        .fn()
        .mockResolvedValue([
          { account_id: 'gmail_test', email: 'test@example.com', account_status: 'active' },
        ]),
    },
    statements: {
      listHistory: vi.fn().mockResolvedValue([]),
    },
    ingestion: {
      startHistoricalScan: (...args: unknown[]) => startHistoricalScanMock(...args),
      cancelScan: (...args: unknown[]) => cancelScanMock(...args),
      getFailedScanCount: vi.fn().mockResolvedValue(0),
      retryFailedScan: vi.fn().mockResolvedValue('started'),
      getScanStatus: (...args: unknown[]) => getScanStatusMock(...args),
    },
  },
}));

vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({ toast: vi.fn() }),
}));

const listenHandlers: Record<string, (event: { payload: unknown }) => void> = {};
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    listenHandlers[event] = handler;
    return Promise.resolve(() => {
      delete listenHandlers[event];
    });
  }),
}));

function Probe() {
  const {
    scanStatus,
    scanError,
    scanProgress,
    scanStartedAt,
    scanFinishedAt,
    connectedAccounts,
    handleStartScan,
    handleCancelScan,
    resetScan,
  } = useGlobalState();
  return (
    <div>
      <div data-testid="connected-count">{connectedAccounts.length}</div>
      <div data-testid="scan-status">{scanStatus}</div>
      <div data-testid="scan-error">{String(scanError)}</div>
      <div data-testid="scan-processed">{String(scanProgress?.processed)}</div>
      <div data-testid="scan-started-at">{String(scanStartedAt)}</div>
      <div data-testid="scan-finished-at">{String(scanFinishedAt)}</div>
      <button onClick={handleStartScan}>start</button>
      <button onClick={handleCancelScan}>cancel</button>
      <button onClick={resetScan}>reset</button>
    </div>
  );
}

function renderProvider() {
  return render(
    <GlobalStateProvider>
      <Probe />
    </GlobalStateProvider>
  );
}

async function startScan() {
  await waitFor(() => expect(screen.getByTestId('connected-count').textContent).toBe('1'));
  await act(async () => {
    screen.getByText('start').click();
  });
  await waitFor(() => expect(startHistoricalScanMock).toHaveBeenCalled());
}

describe('GlobalStateContext scan cancellation + timing', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    cancelScanMock.mockResolvedValue('cancel_requested');
    startHistoricalScanMock.mockResolvedValue('started');
    getScanStatusMock.mockResolvedValue(NOT_STARTED);
    for (const k of Object.keys(listenHandlers)) delete listenHandlers[k];
  });

  it('records a start timestamp when a scan starts', async () => {
    renderProvider();
    const before = Date.now();
    await startScan();

    expect(screen.getByTestId('scan-status').textContent).toBe('running');
    expect(Number(screen.getByTestId('scan-started-at').textContent)).toBeGreaterThanOrEqual(
      before
    );
  });

  it('test_scan_cancelled_event_sets_cancelled_status_not_done_or_error', async () => {
    renderProvider();
    await startScan();
    await waitFor(() => expect(listenHandlers['scan_cancelled']).toBeDefined());

    act(() => {
      listenHandlers['scan_cancelled']({
        payload: {
          account_id: 'gmail_test',
          processed: 3,
          total: 10,
          transactions_found: 1,
          statements_found: 0,
          mandate_events_found: 0,
          non_financial: 2,
          errors: 0,
          error_message: null,
        },
      });
    });

    expect(screen.getByTestId('scan-status').textContent).toBe('cancelled');
    expect(screen.getByTestId('scan-finished-at').textContent).not.toBe('null');
  });

  it('handleCancelScan calls the cancel IPC command for the connected account', async () => {
    renderProvider();
    await startScan();

    await act(async () => {
      screen.getByText('cancel').click();
    });

    expect(cancelScanMock).toHaveBeenCalledWith('gmail_test');
  });

  it('resetScan clears status, progress, and timestamps back to idle', async () => {
    renderProvider();
    await startScan();
    // waitFor rather than a single read: under a loaded parallel run the
    // provider's async effects can settle a tick later (this test was flaky).
    await waitFor(() => expect(screen.getByTestId('scan-status').textContent).toBe('running'));

    await act(async () => {
      screen.getByText('reset').click();
    });

    await waitFor(() => expect(screen.getByTestId('scan-status').textContent).toBe('idle'));
    expect(screen.getByTestId('scan-started-at').textContent).toBe('null');
    expect(screen.getByTestId('scan-finished-at').textContent).toBe('null');
  });

  // Review P8: the panel follows the backend, not only the Start button.
  it('adopts a scan already running in the backend on mount', async () => {
    getScanStatusMock.mockResolvedValue({
      ...NOT_STARTED,
      status: 'in_progress',
      processed: 40,
      total: 100,
    });
    renderProvider();
    await waitFor(() => expect(screen.getByTestId('scan-status').textContent).toBe('running'));
    expect(screen.getByTestId('scan-processed').textContent).toBe('40');
    expect(getScanStatusMock).toHaveBeenCalledWith('gmail_test');
  });

  it('follows a scan the backend started on its own', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['scan_progress']).toBeDefined());
    act(() => {
      listenHandlers['scan_progress']({
        payload: {
          ...NOT_STARTED,
          account_id: 'gmail_test',
          processed: 7,
          total: 20,
          non_financial: 0,
        },
      });
    });
    expect(screen.getByTestId('scan-status').textContent).toBe('running');
    expect(screen.getByTestId('scan-processed').textContent).toBe('7');
  });

  it('ignores events for another account', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['scan_progress']).toBeDefined());
    act(() => {
      listenHandlers['scan_progress']({
        payload: { ...NOT_STARTED, account_id: 'other', processed: 7, total: 20, non_financial: 0 },
      });
    });
    expect(screen.getByTestId('scan-status').textContent).toBe('idle');
  });

  it('shows the error_message sent with scan_failed', async () => {
    renderProvider();
    await startScan();
    await waitFor(() => expect(listenHandlers['scan_failed']).toBeDefined());
    act(() => {
      listenHandlers['scan_failed']({
        payload: {
          ...NOT_STARTED,
          account_id: 'gmail_test',
          non_financial: 0,
          errors: 1,
          error_message: 'token expired',
        },
      });
    });
    expect(screen.getByTestId('scan-status').textContent).toBe('error');
    expect(screen.getByTestId('scan-error').textContent).toBe('token expired');
  });
});
