// Covers the Mail Scan pane after it was split out of the old 797-line
// Settings.tsx: the section, its progress panel, and its controls.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react';
import MailScanSection from '@/pages/settings/MailScanSection';
import { useGlobalState } from '@/lib/GlobalStateContext';
import type { ScanProgressPayload } from '@/lib/ipc';
import type { ScanStatus } from '@/lib/GlobalStateContext';

vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const handleStartScan = vi.fn();
const handleCancelScan = vi.fn();
const resetScan = vi.fn();

const progress = (over: Partial<ScanProgressPayload> = {}): ScanProgressPayload => ({
  account_id: 'acc1',
  processed: 40,
  total: 100,
  transactions_found: 7,
  statements_found: 2,
  mandate_events_found: 0,
  non_financial: 30,
  errors: 0,
  pending_enrichment: 0,
  ...over,
});

function mockState(over: Record<string, unknown> = {}) {
  asMock(useGlobalState).mockImplementation(() => ({
    scanStartDate: '2026-01-01',
    setScanStartDate: vi.fn(),
    scanEndDate: '2026-02-01',
    setScanEndDate: vi.fn(),
    scanStatus: 'idle' as ScanStatus,
    scanProgress: null,
    scanStartedAt: null,
    scanFinishedAt: null,
    scanError: null,
    connectedAccounts: [{ account_id: 'acc1', email: 'user@gmail.com' }],
    handleStartScan,
    handleCancelScan,
    resetScan,
    ...over,
  }));
}

beforeEach(() => {
  vi.clearAllMocks();
  mockState();
});

describe('MailScanSection', () => {
  it('warns and blocks the scan when no account is connected', () => {
    mockState({ connectedAccounts: [] });
    render(<MailScanSection />);
    expect(screen.getByText(/Connect a Gmail account above/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Start Scan/ })).toBeDisabled();
  });

  it('names the account it will scan and enables the scan once connected', () => {
    render(<MailScanSection />);
    expect(screen.getByText('user@gmail.com')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Start Scan/ })).toBeEnabled();
    fireEvent.click(screen.getByRole('button', { name: /Start Scan/ }));
    expect(handleStartScan).toHaveBeenCalled();
  });

  it('blocks the scan when either end of the date range is empty', () => {
    mockState({ scanEndDate: '' });
    render(<MailScanSection />);
    expect(screen.getByRole('button', { name: /Start Scan/ })).toBeDisabled();
  });

  it('shows no progress panel before a scan has run', () => {
    render(<MailScanSection />);
    expect(screen.queryByText(/Scanning emails/)).not.toBeInTheDocument();
  });
});

describe('MailScanSection progress panel', () => {
  it('reports live counts and elapsed time while running', () => {
    mockState({
      scanStatus: 'running',
      scanProgress: progress(),
      scanStartedAt: Date.now() - 30_000,
    });
    render(<MailScanSection />);
    expect(screen.getByText(/Scanning emails/)).toBeInTheDocument();
    expect(screen.getByText(/\(40 \/ 100\)/)).toBeInTheDocument();
    expect(screen.getByText(/Elapsed:/)).toBeInTheDocument();
    // Stat tiles
    expect(screen.getByText('7')).toBeInTheDocument();
    expect(screen.getByText('30')).toBeInTheDocument();
  });

  it('freezes the duration with success wording once the scan completes', () => {
    mockState({
      scanStatus: 'done',
      scanProgress: progress({ processed: 100 }),
      scanStartedAt: 1000,
      scanFinishedAt: 61_000,
    });
    render(<MailScanSection />);
    expect(screen.getByText(/Scan complete!/)).toBeInTheDocument();
    expect(screen.getByText(/Completed in/)).toBeInTheDocument();
  });

  it('uses distinct cancelled wording and neutral duration phrasing', () => {
    mockState({
      scanStatus: 'cancelled',
      scanProgress: progress(),
      scanStartedAt: 1000,
      scanFinishedAt: 31_000,
    });
    render(<MailScanSection />);
    expect(screen.getByText(/Scan cancelled\./)).toBeInTheDocument();
    expect(screen.getByText(/Ran for/)).toBeInTheDocument();
  });

  it('surfaces the failure message on error', () => {
    mockState({
      scanStatus: 'error',
      scanProgress: progress({ errors: 3 }),
      scanError: 'Gmail rate limit exceeded',
    });
    render(<MailScanSection />);
    expect(screen.getByText(/Scan failed\./)).toBeInTheDocument();
    expect(screen.getByText('Gmail rate limit exceeded')).toBeInTheDocument();
    expect(screen.getByText('3')).toBeInTheDocument();
  });
});

describe('MailScanSection controls', () => {
  it('offers Clear after a finished scan and not while running', () => {
    mockState({ scanStatus: 'running', scanProgress: progress() });
    const { unmount } = render(<MailScanSection />);
    expect(screen.queryByRole('button', { name: 'Clear' })).not.toBeInTheDocument();
    unmount();

    mockState({ scanStatus: 'done', scanProgress: progress() });
    render(<MailScanSection />);
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(resetScan).toHaveBeenCalled();
  });

  it('only offers Cancel while a scan is running', () => {
    render(<MailScanSection />);
    expect(screen.queryByRole('button', { name: /Cancel Scan/ })).not.toBeInTheDocument();
  });

  it('confirms in-app before cancelling, and only then calls the IPC', async () => {
    mockState({ scanStatus: 'running', scanProgress: progress() });
    render(<MailScanSection />);

    fireEvent.click(screen.getByRole('button', { name: /Cancel Scan/ }));
    expect(handleCancelScan).not.toHaveBeenCalled();

    const dialog = await screen.findByRole('dialog');
    expect(dialog).toHaveTextContent(/Cancel the in-progress scan\?/);

    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel Scan' }));
    await waitFor(() => expect(handleCancelScan).toHaveBeenCalled());
  });

  it('leaves the scan alone when the confirm is dismissed', async () => {
    mockState({ scanStatus: 'running', scanProgress: progress() });
    render(<MailScanSection />);

    fireEvent.click(screen.getByRole('button', { name: /Cancel Scan/ }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Keep Scanning' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(handleCancelScan).not.toHaveBeenCalled();
  });
});
