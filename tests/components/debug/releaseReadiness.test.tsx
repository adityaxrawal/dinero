// Release readiness deliberately separates what this app can verify locally
// from the out-of-repo licensing backend; that framing is what these pin.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { ReleaseReadinessViewer } from '@/components/debug/ReleaseReadinessViewer';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({
  API: {
    debug: {
      listReleaseReadinessSnapshots: vi.fn(),
      captureReleaseReadinessSnapshot: vi.fn(),
    },
  },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const metrics = {
  total_transactions: 1200,
  total_statements: 14,
  unresolved_clusters: 3,
  llm_fallback_rate: 0.042,
  queue_depth: 0,
  extraction_layer_distribution: {},
  reconciliation_decision_distribution: {},
};

const snapshot = (over = {}) => ({
  id: 's1',
  captured_at: '2026-08-01T10:00:00Z',
  go_no_go: true,
  metrics,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  asMock(API.debug.listReleaseReadinessSnapshots).mockResolvedValue([]);
  asMock(API.debug.captureReleaseReadinessSnapshot).mockResolvedValue(undefined);
});

describe('ReleaseReadinessViewer', () => {
  it('explains that no snapshot exists rather than showing a fake verdict', async () => {
    render(<ReleaseReadinessViewer metrics={null} />);
    expect(await screen.findByText(/No snapshot yet/)).toBeInTheDocument();
  });

  it('reports a GO verdict from the newest snapshot', async () => {
    asMock(API.debug.listReleaseReadinessSnapshots).mockResolvedValue([snapshot()]);
    render(<ReleaseReadinessViewer metrics={null} />);
    expect(await screen.findByText(/^GO — last captured/)).toBeInTheDocument();
  });

  it('reports a NO-GO verdict just as plainly', async () => {
    asMock(API.debug.listReleaseReadinessSnapshots).mockResolvedValue([
      snapshot({ go_no_go: false }),
    ]);
    render(<ReleaseReadinessViewer metrics={null} />);
    expect(await screen.findByText(/^NO-GO — last captured/)).toBeInTheDocument();
  });

  it('flags a metric that regressed against the previous snapshot', async () => {
    asMock(API.debug.listReleaseReadinessSnapshots).mockResolvedValue([
      snapshot({ id: 'new', metrics: { ...metrics, unresolved_clusters: 9 } }),
      snapshot({ id: 'old', metrics: { ...metrics, unresolved_clusters: 1 } }),
    ]);
    render(<ReleaseReadinessViewer metrics={null} />);
    expect(await screen.findByText(/Regressed vs\. previous snapshot/)).toBeInTheDocument();
  });

  it('captures a snapshot and then re-reads the list', async () => {
    render(<ReleaseReadinessViewer metrics={null} />);
    fireEvent.click(await screen.findByRole('button', { name: 'Capture Snapshot' }));
    await waitFor(() =>
      expect(API.debug.captureReleaseReadinessSnapshot).toHaveBeenCalled()
    );
    await waitFor(() =>
      expect(API.debug.listReleaseReadinessSnapshots).toHaveBeenCalledTimes(2)
    );
  });

  it('shows the locally-measured metrics, or a loading note without them', async () => {
    const { unmount } = render(<ReleaseReadinessViewer metrics={null} />);
    expect(await screen.findByText('Loading...')).toBeInTheDocument();
    unmount();

    render(<ReleaseReadinessViewer metrics={metrics as never} />);
    expect(await screen.findByText('1200')).toBeInTheDocument();
    expect(screen.getByText('4.2%')).toBeInTheDocument();
  });

  it('presents the quality gates as declared targets, not live numbers', async () => {
    render(<ReleaseReadinessViewer metrics={null} />);
    expect(await screen.findByText(/declared targets, not a live/)).toBeInTheDocument();
    expect(screen.getByText('≥ 95%')).toBeInTheDocument();
  });

  it('states plainly that the licensing backend is out of this repository', async () => {
    render(<ReleaseReadinessViewer metrics={null} />);
    expect(await screen.findByText(/not part of this repository/)).toBeInTheDocument();
  });
});
