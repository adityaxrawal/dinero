import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import { GlobalStateProvider, useGlobalState } from './GlobalStateContext';

const cancelScanMock = vi.fn().mockResolvedValue('cancel_requested');
const startHistoricalScanMock = vi.fn().mockResolvedValue('started');

vi.mock('@/lib/ipc', () => ({
  API: {
    auth: {
      listConnectedAccounts: vi.fn().mockResolvedValue([
        { account_id: 'gmail_test', email: 'test@example.com', account_status: 'active' },
      ]),
    },
    statements: {
      listHistory: vi.fn().mockResolvedValue([]),
    },
    ingestion: {
      startHistoricalScan: (...args: unknown[]) => startHistoricalScanMock(...args),
      cancelScan: (...args: unknown[]) => cancelScanMock(...args),
    },
  },
}));

vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({ toast: vi.fn() }),
}));

const listenHandlers: Record<string, (event: { payload: any }) => void> = {};
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: any }) => void) => {
    listenHandlers[event] = handler;
    return Promise.resolve(() => {
      delete listenHandlers[event];
    });
  }),
}));

function Probe() {
  const {
    scanStatus,
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
    expect(screen.getByTestId('scan-status').textContent).toBe('running');

    await act(async () => {
      screen.getByText('reset').click();
    });

    expect(screen.getByTestId('scan-status').textContent).toBe('idle');
    expect(screen.getByTestId('scan-started-at').textContent).toBe('null');
    expect(screen.getByTestId('scan-finished-at').textContent).toBe('null');
  });
});
