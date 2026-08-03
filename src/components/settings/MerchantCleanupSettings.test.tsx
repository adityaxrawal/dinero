import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act, fireEvent } from '@testing-library/react';
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

vi.mock('@/hooks/use-toast', () => ({ toast: vi.fn() }));
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


/**
 * The idle panel: the candidate queue it offers to clean, the history of past
 * runs, and the two write paths (start a run, undo one). None of this is
 * reachable from the mid-run tests above, which never leave the running state.
 */
describe('MerchantCleanupSettings, idle', () => {
  const change = (over = {}) => ({
    correction_id: 'corr1',
    transaction_id: 'tx1',
    bank_name: 'Yes Bank',
    previous_merchant: 'PYU*Swiggy Food',
    new_merchant: 'Swiggy',
    category: 'Food & Dining',
    confidence: 0.94,
    reverted: false,
    ...over,
  });

  const run = (over = {}) => ({
    run_id: 'run_1',
    started_at: '2026-07-29 10:00:00',
    applied: 3,
    reverted: 0,
    banks: ['Yes Bank'],
    changes: [change()],
    ...over,
  });

  let api: typeof import('@/lib/ipc').API;

  const mount = async () => {
    const { default: MerchantCleanupSettings } = await import('./MerchantCleanupSettings');
    render(<MerchantCleanupSettings />);
    await waitFor(() =>
      expect(screen.getByText(/merchant names Dinero isn't sure about/)).toBeInTheDocument()
    );
  };

  /** The sample list sits behind the "What is in the queue" disclosure. */
  const expandQueue = async () => {
    await act(async () => {
      screen.getByText('What is in the queue').closest('button')!.click();
    });
  };

  const withPreview = (over: Record<string, unknown>) =>
    vi.mocked(api.merchantCleanup.preview).mockResolvedValue({ ...preview, ...over } as never);

  beforeEach(async () => {
    emitProgress = null;
    vi.clearAllMocks();
    ({ API: api } = await import('@/lib/ipc'));
    vi.mocked(api.merchantCleanup.preview).mockResolvedValue(preview as never);
    vi.mocked(api.merchantCleanup.runs).mockResolvedValue([] as never);
    vi.mocked(api.merchantCleanup.start).mockResolvedValue('run_1' as never);
    vi.mocked(api.merchantCleanup.revert).mockResolvedValue(3 as never);
  });

  describe('candidate queue', () => {
    it('shows the worst-scoring merchant up front', async () => {
      await mount();
      expect(screen.getByText('Worst match')).toBeInTheDocument();
      expect(screen.getAllByText('PYU*Swiggy Food').length).toBeGreaterThan(0);
    });

    it('lists a candidate with its formatted amount once expanded', async () => {
      await mount();
      await expandQueue();
      expect(screen.getByText(/245\.43/)).toBeInTheDocument();
    });

    it('marks a credit with a leading plus', async () => {
      withPreview({ samples: [{ ...preview.samples[0], direction: 'credit' }] });
      await mount();
      await expandQueue();
      expect(screen.getByText(/\+.*245\.43/)).toBeInTheDocument();
    });

    it('omits the amount when none was extracted', async () => {
      withPreview({ samples: [{ ...preview.samples[0], amount: null }] });
      await mount();
      await expandQueue();
      expect(screen.queryByText(/245\.43/)).toBeNull();
    });

    it('warns when the original email is no longer stored', async () => {
      withPreview({ samples: [{ ...preview.samples[0], has_evidence: false }] });
      await mount();
      await expandQueue();
      expect(screen.getByText(/no email kept/)).toBeInTheDocument();
    });

    it('says how many more are queued beyond the shown sample', async () => {
      await mount();
      await expandQueue();
      expect(screen.getByText(/and 383 more/)).toBeInTheDocument();
    });
  });

  describe('starting a run', () => {
    it('asks the backend to start and switches into the running state', async () => {
      await mount();
      await act(async () => {
        screen.getByRole('button', { name: /Normalize with AI/ }).click();
      });
      expect(api.merchantCleanup.start).toHaveBeenCalled();
      await waitFor(() => expect(screen.getByRole('button', { name: /Stop/ })).toBeInTheDocument());
    });

    it('surfaces a start failure instead of pretending the run began', async () => {
      vi.mocked(api.merchantCleanup.start).mockRejectedValue(new Error('model not loaded'));
      await mount();
      await act(async () => {
        screen.getByRole('button', { name: /Normalize with AI/ }).click();
      });
      expect(await screen.findByText('model not loaded')).toBeInTheDocument();
    });
  });

  describe('run history', () => {
    const mountWithRuns = async (...runs: ReturnType<typeof run>[]) => {
      vi.mocked(api.merchantCleanup.runs).mockResolvedValue(runs as never);
      await mount();
    };

    /**
     * Undoing is two steps: the row button opens a confirmation dialog whose
     * confirm button carries the same "Undo run" label, so the dialog's copy
     * is the one that actually reverts.
     */
    const confirmUndo = async () => {
      const rowButton = await screen.findByRole('button', { name: /Undo run/ });
      await act(async () => { fireEvent.click(rowButton); });
      const buttons = await screen.findAllByRole('button', { name: /Undo run/ });
      await act(async () => { fireEvent.click(buttons[buttons.length - 1]); });
    };

    it('summarises a past run', async () => {
      await mountWithRuns(run());
      expect(await screen.findByText(/3 still applied/)).toBeInTheDocument();
    });

    it('mentions how many changes were already undone', async () => {
      await mountWithRuns(run({ reverted: 2 }));
      expect(await screen.findByText(/2 undone/)).toBeInTheDocument();
    });

    it('offers no undo for a fully reverted run', async () => {
      await mountWithRuns(run({ applied: 0, reverted: 3 }));
      expect(await screen.findByText('Already undone')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /Undo run/ })).toBeNull();
    });

    it('reveals the individual changes when expanded', async () => {
      await mountWithRuns(run());
      const summary = await screen.findByText(/3 still applied/);
      await act(async () => summary.closest('button')!.click());
      expect(screen.getByText('Swiggy')).toBeInTheDocument();
    });

    it('undoes a whole run and confirms with a pluralised toast', async () => {
      const { toast } = await import('@/hooks/use-toast');
      await mountWithRuns(run());
      await confirmUndo();
      expect(api.merchantCleanup.revert).toHaveBeenCalledWith('run_1');
      await waitFor(() =>
        expect(vi.mocked(toast)).toHaveBeenCalledWith(
          expect.objectContaining({ title: 'Undid 3 corrections' })
        )
      );
    });

    it('uses the singular form when a run undid one correction', async () => {
      const { toast } = await import('@/hooks/use-toast');
      vi.mocked(api.merchantCleanup.revert).mockResolvedValue(1 as never);
      await mountWithRuns(run());
      await confirmUndo();
      await waitFor(() =>
        expect(vi.mocked(toast)).toHaveBeenCalledWith(
          expect.objectContaining({ title: 'Undid 1 correction' })
        )
      );
    });

    it('surfaces an undo failure', async () => {
      vi.mocked(api.merchantCleanup.revert).mockRejectedValue(new Error('db locked'));
      await mountWithRuns(run());
      await confirmUndo();
      expect(await screen.findByText('db locked')).toBeInTheDocument();
    });
  });
});
