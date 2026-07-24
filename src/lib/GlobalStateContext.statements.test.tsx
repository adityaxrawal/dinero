// Doc 30 TASK-RT-008 acceptance: test_single_upload_shows_summary_toast,
// test_batch_upload_aggregates_into_single_summary,
// test_password_required_uses_modal_not_toast.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { GlobalStateProvider, useGlobalState } from './GlobalStateContext';

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
  const { passwordModalOpen } = useGlobalState();
  return <div data-testid="password-modal-open">{String(passwordModalOpen)}</div>;
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

  it('test_password_required_uses_modal_not_toast: opens the password modal, never a toast', async () => {
    renderProvider();
    await waitFor(() => expect(listenHandlers['statement_password_required']).toBeDefined());

    listenHandlers['statement_password_required']({
      payload: { statementId: 'stmt_locked', instrumentId: 'inst_1' },
    });

    await waitFor(() => {
      expect(screen.getByTestId('password-modal-open').textContent).toBe('true');
    });
    expect(toastSpy).not.toHaveBeenCalled();
  });
});
