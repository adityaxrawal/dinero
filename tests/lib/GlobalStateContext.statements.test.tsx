// Doc 30 TASK-RT-008 acceptance: test_single_upload_shows_summary_toast,
// test_batch_upload_aggregates_into_single_summary.
// Doc 2026-07-26 mail scan performance: statement_password_required no
// longer auto-opens the modal -- it's batched into one debounced toast.
import { useEffect } from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, act } from '@testing-library/react';
import { listen } from '@tauri-apps/api/event';
import { GlobalStateProvider, useGlobalState } from '@/lib/GlobalStateContext';

vi.mock('@/lib/ipc', () => ({
  API: {
    statements: {
      listHistory: vi.fn().mockResolvedValue([]),
    },
  },
}));

const toastSpy = vi.fn();
vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({ toast: toastSpy }),
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

let watchDraftOrigin: (originId: string) => void = () => {};

function Probe() {
  const state = useGlobalState();
  // Captured in an effect, not during render: reassigning a module-level
  // binding while rendering is the side effect react-hooks/globals forbids.
  useEffect(() => {
    watchDraftOrigin = state.watchDraftOrigin;
  }, [state.watchDraftOrigin]);
  return (
    <>
      <div data-testid="password-modal-open">{String(state.passwordModalOpen)}</div>
      <div data-testid="instrument-modal-open">{String(state.instrumentModalOpen)}</div>
      <div data-testid="instrument-filename">{state.pendingInstrumentFilename}</div>
      <div data-testid="instrument-reason">{state.pendingInstrumentReason}</div>
      <div data-testid="review-modal-open">{String(state.reviewModalOpen)}</div>
      <div data-testid="active-draft">{state.activeDraftId ?? ''}</div>
      <div data-testid="processing-progress">{state.processingProgress?.draft_id ?? ''}</div>
    </>
  );
}

function renderProvider() {
  return render(
    <GlobalStateProvider>
      <Probe />
    </GlobalStateProvider>
  );
}

describe('GlobalStateContext statement notifications', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    for (const k of Object.keys(listenHandlers)) delete listenHandlers[k];
  });

  it('test_single_upload_shows_summary_toast: a single statement_parsed event shows a real summary, not a generic message', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_parsed']).toBeDefined());

    listenHandlers['statement_parsed']({
      payload: {
        statement_id: 'stmt_1',
        instrument_id: 'inst_1',
        issuer_name: 'HDFC Card',
        rows_extracted: 47,
      },
    });

    expect(toastSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Statement Parsed',
        description: expect.stringContaining('HDFC Card statement parsed — 47 transactions found'),
      })
    );
  });

  it('a failed statement outside a batch shows a failure toast linking to the retry panel', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_parse_failed']).toBeDefined());

    listenHandlers['statement_parse_failed']({
      payload: { reason: 'Incorrect password', filename: 'axis_statement.pdf' },
    });

    expect(toastSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Parse Failed',
        description: expect.stringContaining('axis_statement.pdf'),
        actionTo: '/statements',
      })
    );
  });

  it('test_batch_upload_aggregates_into_single_summary: individual events during an active batch are suppressed, one aggregate toast fires on completion', async () => {
    renderProvider();
    await waitFor(() => {
      expect(listenHandlers['statement_batch_progress']).toBeDefined();
      expect(listenHandlers['statement_parsed']).toBeDefined();
      expect(listenHandlers['statement_parse_failed']).toBeDefined();
    });

    // Batch starts (first progress tick).
    listenHandlers['statement_batch_progress']({
      payload: { parsed: 0, total: 10, eta_seconds: 30 },
    });

    // 8 succeed, 2 fail -- none of these should toast individually.
    for (let i = 0; i < 8; i++) {
      listenHandlers['statement_parsed']({ payload: { statement_id: `s${i}` } });
    }
    listenHandlers['statement_parse_failed']({
      payload: { reason: 'Incorrect password', filename: 'a.pdf' },
    });
    listenHandlers['statement_parse_failed']({
      payload: { reason: 'Incorrect password', filename: 'b.pdf' },
    });
    expect(toastSpy).not.toHaveBeenCalled();

    // Batch completes.
    listenHandlers['statement_batch_progress']({
      payload: { parsed: 10, total: 10, eta_seconds: 0 },
    });

    expect(toastSpy).toHaveBeenCalledTimes(1);
    expect(toastSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        title: 'Batch Import Complete',
        description: expect.stringContaining('8/10 imported, 2 failed'),
      })
    );
  });

  it('statement_password_required never auto-opens the modal', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_password_required']).toBeDefined());

    listenHandlers['statement_password_required']({
      payload: { statementId: 'stmt_locked', instrumentId: 'inst_1' },
    });

    expect(screen.getByTestId('password-modal-open').textContent).toBe('false');
  });

  it('batches multiple statement_password_required events into one toast and does not open the modal', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_password_required']).toBeDefined());

    vi.useFakeTimers();
    try {
      // Simulate the backend firing the event 3 times in quick succession
      // (matches the real burst behavior during a historical scan, per
      // `resolve_statement_password`'s doc comment).
      listenHandlers['statement_password_required']({ payload: { statementId: 'stmt_1' } });
      listenHandlers['statement_password_required']({ payload: { statementId: 'stmt_2' } });
      listenHandlers['statement_password_required']({ payload: { statementId: 'stmt_3' } });

      expect(screen.getByTestId('password-modal-open').textContent).toBe('false');
      expect(toastSpy).not.toHaveBeenCalled();

      // Advance past the debounce window (~2s) and confirm exactly one
      // toast fired, mentioning the batched count.
      await act(async () => {
        vi.advanceTimersByTime(2100);
      });

      expect(toastSpy).toHaveBeenCalledTimes(1);
      expect(toastSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          description: expect.stringContaining('3 statements need a password'),
        })
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it('releases every statement listener on unmount', async () => {
    const { unmount } = renderProvider();
    await waitFor(() => expect(listenHandlers['statement_staged']).toBeDefined());

    unmount();
    await waitFor(() => expect(Object.keys(listenHandlers)).toHaveLength(0));
  });

  it('warns once when a statement is rejected as a duplicate', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_duplicate_rejected']).toBeDefined());

    act(() => listenHandlers['statement_duplicate_rejected']({ payload: {} }));

    expect(toastSpy).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Duplicate Statement', variant: 'destructive' })
    );
  });

  it('opens the instrument gate with the payload the backend sent', async () => {
    renderProvider();
    await waitFor(() =>
      expect(listenHandlers['statement_instrument_confirmation_required']).toBeDefined()
    );

    act(() =>
      listenHandlers['statement_instrument_confirmation_required']({
        payload: {
          statement_id: 'stmt_9',
          filename: 'hdfc-april.pdf',
          issuer: 'HDFC',
          reason: 'Account number unreadable',
        },
      })
    );

    await waitFor(() =>
      expect(screen.getByTestId('instrument-modal-open')).toHaveTextContent('true')
    );
    expect(screen.getByTestId('instrument-filename')).toHaveTextContent('hdfc-april.pdf');
    expect(screen.getByTestId('instrument-reason')).toHaveTextContent('Account number unreadable');
  });

  it('falls back to a generic reason when the backend omits one', async () => {
    renderProvider();
    await waitFor(() =>
      expect(listenHandlers['statement_instrument_confirmation_required']).toBeDefined()
    );

    act(() =>
      listenHandlers['statement_instrument_confirmation_required']({
        payload: { statement_id: 'stmt_9' },
      })
    );

    await waitFor(() =>
      expect(screen.getByTestId('instrument-reason')).toHaveTextContent(
        'could not be read automatically'
      )
    );
    expect(screen.getByTestId('instrument-filename')).toHaveTextContent('');
  });

  it('tracks processing progress only for drafts this client is watching', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_processing_progress']).toBeDefined());

    // Unwatched (e.g. a background email scan): ignored, so an unrelated
    // upload cannot drive this client's review modal progress bar.
    act(() =>
      listenHandlers['statement_processing_progress']({ payload: { draft_id: 'other_draft' } })
    );
    expect(screen.getByTestId('processing-progress')).toHaveTextContent('');

    act(() => watchDraftOrigin('my_draft'));
    act(() =>
      listenHandlers['statement_processing_progress']({ payload: { draft_id: 'my_draft' } })
    );

    await waitFor(() =>
      expect(screen.getByTestId('processing-progress')).toHaveTextContent('my_draft')
    );
  });

  it('auto-opens review only for a draft whose origin was watched', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_staged']).toBeDefined());

    act(() => watchDraftOrigin('my_draft'));
    act(() =>
      listenHandlers['statement_staged']({ payload: { draft_id: 'my_draft', origin: 'upload' } })
    );

    await waitFor(() => expect(screen.getByTestId('review-modal-open')).toHaveTextContent('true'));
    expect(screen.getByTestId('active-draft')).toHaveTextContent('my_draft');
  });

  it('queues a background-staged draft instead of hijacking the screen', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_staged']).toBeDefined());

    act(() =>
      listenHandlers['statement_staged']({ payload: { draft_id: 'bg_draft', origin: 'email' } })
    );

    expect(screen.getByTestId('review-modal-open')).toHaveTextContent('false');
    expect(toastSpy).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Statement ready for review' })
    );
  });

  it('consumes the watch so a redelivered staged event does not reopen review', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_staged']).toBeDefined());

    act(() => watchDraftOrigin('my_draft'));
    act(() =>
      listenHandlers['statement_staged']({ payload: { draft_id: 'my_draft', origin: 'upload' } })
    );
    await waitFor(() => expect(screen.getByTestId('review-modal-open')).toHaveTextContent('true'));

    toastSpy.mockClear();
    act(() =>
      listenHandlers['statement_staged']({ payload: { draft_id: 'my_draft', origin: 'upload' } })
    );

    // The id was deleted on first use, so the redelivery takes the
    // background branch rather than re-opening the modal.
    expect(toastSpy).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'Statement ready for review' })
    );
  });

  it('registers each statement listener exactly once across re-renders', async () => {
    // `useStatementModals` returns a fresh object literal every render. While
    // the effect depended on that object, every render of the provider tore
    // down and re-registered all eight listeners and re-fired
    // `loadStatementHistory` -- and an event landing between teardown and
    // re-registration was dropped. Memoizing the object would not have fixed
    // it: it still changes whenever any modal opens or closes.
    const statementRegistrations = () =>
      vi.mocked(listen).mock.calls.filter(([event]) => String(event).startsWith('statement_'))
        .length;

    const { rerender } = renderProvider();
    await waitFor(() => expect(listenHandlers['statement_staged']).toBeDefined());
    expect(statementRegistrations()).toBe(8);

    rerender(
      <GlobalStateProvider>
        <Probe />
      </GlobalStateProvider>
    );
    await act(async () => {
      await Promise.resolve();
    });

    expect(statementRegistrations()).toBe(8);
  });

  it('releases listeners that finish registering after an unmount', async () => {
    // Registration is async, so unmounting before it settles used to leave
    // every listener attached for the life of the process: the cleanup ran
    // against `unlisten` variables that were all still undefined.
    const { unmount } = renderProvider();
    unmount();

    await act(async () => {
      await Promise.resolve();
    });
    await waitFor(() => expect(Object.keys(listenHandlers)).toHaveLength(0));
  });
});
