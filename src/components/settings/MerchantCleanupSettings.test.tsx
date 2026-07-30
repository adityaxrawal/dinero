import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import type { MerchantCleanupProgress } from '@/lib/ipc';

/**
 * The running state is the defect this section was rebuilt around: it used to
 * hide the whole queue behind `!isRunning` and show a bare progress bar, so a
 * multi-minute run looked like a hung screen. These tests pin the things that
 * replaced it — measured counters, a derived ETA, and the model's answers as
 * they land — because none of them are visible in a static render.
 */

/** Captured `useIpcListen` handler, so a test can drive progress events. */
let emitProgress: ((p: MerchantCleanupProgress) => void) | null = null;

vi.mock('@/hooks/useIpcListen', () => ({
  useIpcListen: (_event: string, handler: (p: MerchantCleanupProgress) => void) => {
    emitProgress = handler;
  },
}));

const preview = {
  candidate_count: 384,
  no_evidence_count: 12,
  by_bank: [{ bank_name: 'Yes Bank', count: 384, no_evidence: 12 }],
  samples: [
    {
      transaction_id: 'tx1',
      merchant: 'PYU*Swiggy Food',
      bank_name: 'Yes Bank',
      confidence: 0.12,
      has_evidence: true,
      amount: 245.43,
      currency: 'INR',
      direction: 'debit',
      event_time: '2026-07-29 10:00:00',
    },
  ],
  llm_eligible: true,
  total_ram_gb: 18,
  running: false,
};

vi.mock('@/lib/ipc', () => ({
  API: {
    merchantCleanup: {
      preview: vi.fn(() => Promise.resolve(preview)),
      runs: vi.fn(() => Promise.resolve([])),
      start: vi.fn(() => Promise.resolve('run_1')),
      cancel: vi.fn(() => Promise.resolve()),
      revert: vi.fn(() => Promise.resolve(0)),
      revertCorrection: vi.fn(() => Promise.resolve()),
    },
    llm: {
      getActiveModel: vi.fn(() => Promise.resolve('qwen2.5-7b')),
      getAvailableModels: vi.fn(() => Promise.resolve([{ id: 'qwen2.5-7b', name: 'Qwen2.5 7B' }])),
    },
  },
}));

const tick = (over: Partial<MerchantCleanupProgress> = {}): MerchantCleanupProgress => ({
  run_id: 'run_1',
  processed: 13,
  total: 384,
  applied: 11,
  skipped: 2,
  current_merchant: 'PYU*Swiggy Food',
  bank_name: 'Yes Bank',
  resolved_merchant: 'Swiggy',
  resolved_category: 'Food & Dining',
  status: 'running',
  ...over,
});

async function mountAndRun(...ticks: MerchantCleanupProgress[]) {
  const { default: MerchantCleanupSettings } = await import('./MerchantCleanupSettings');
  render(<MerchantCleanupSettings />);
  await waitFor(() =>
    expect(screen.getByText(/merchant names Dinero isn't sure about/)).toBeInTheDocument()
  );
  for (const t of ticks) {
    act(() => emitProgress?.(t));
  }
}

describe('MerchantCleanupSettings, mid-run', () => {
  beforeEach(() => {
    emitProgress = null;
    vi.clearAllMocks();
  });

  it('reports which bank it is reading, and offers Stop instead of Start', async () => {
    await mountAndRun(tick());
    expect(screen.getByText('Reading Yes Bank alerts…')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Stop/ })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Normalize with AI/ })).toBeNull();
  });

  it('shows fixed and skipped counters and the processed total', async () => {
    await mountAndRun(tick());
    expect(screen.getByText('13 / 384')).toBeInTheDocument();
    expect(screen.getByText('3%')).toBeInTheDocument();
    expect(screen.getByText('11')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
  });

  /**
   * The old panel only ever had a static "roughly 4 min" guess computed from the
   * queue size. A run's real speed depends on the model and the Mac, so the ETA
   * has to come from observed throughput or it is decoration.
   */
  it('derives a rate and an ETA from observed throughput', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      // First tick starts the clock; 60s later 13 rows are done, so the measured
      // rate is 13/min and the remaining 371 imply ceil(371/13) = 29 min.
      // Asserting the real arithmetic, not just that the labels rendered — with
      // no elapsed time both fields legitimately read "—".
      await mountAndRun(tick({ processed: 1, applied: 1, skipped: 0 }));
      await act(async () => {
        vi.advanceTimersByTime(60_000);
      });
      act(() => emitProgress?.(tick({ processed: 13, applied: 11, skipped: 2 })));

      expect(screen.getByText('13.0/min')).toBeInTheDocument();
      expect(screen.getByText('~29 min')).toBeInTheDocument();
      expect(screen.getByText(/elapsed 1:00/)).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  /** A counter cannot tell you the answers are sane; the before→after can. */
  it('shows the model answers as they land, newest first', async () => {
    await mountAndRun(
      tick({
        processed: 12,
        current_merchant: 'RAZ*UBER IND',
        resolved_merchant: 'Uber',
        resolved_category: 'Transport',
      }),
      tick()
    );
    expect(screen.getByText('Swiggy')).toBeInTheDocument();
    expect(screen.getByText('Food & Dining')).toBeInTheDocument();
    expect(screen.getByText('Uber')).toBeInTheDocument();

    const feed = screen.getAllByText(/Swiggy|Uber/).map((n) => n.textContent);
    expect(feed.indexOf('Swiggy')).toBeLessThan(feed.indexOf('Uber'));
  });

  it('says a transaction was left alone rather than silently dropping it', async () => {
    await mountAndRun(tick({ resolved_merchant: null, resolved_category: null }));
    expect(screen.getByText(/left alone/)).toBeInTheDocument();
  });

  it('keeps the fixes and explains itself when stopped early', async () => {
    await mountAndRun(tick(), tick({ status: 'cancelled', current_merchant: null }));
    expect(screen.getByText(/Stopped early/)).toBeInTheDocument();
    expect(screen.getByText(/Fixed 11 of 13/)).toBeInTheDocument();
  });
});
