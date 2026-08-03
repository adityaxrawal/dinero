import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import UnprocessedItemsQueue from './UnprocessedItemsQueue';

const toast = vi.fn();
const reparseMutate = vi.fn();
const retryMutate = vi.fn();
const discardMutate = vi.fn();
const openReviewModal = vi.fn();
let groups: Record<string, unknown[]> | undefined;

vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: () => ({ openReviewModal }) }));
vi.mock('@/hooks/useIpcListen', () => ({ useIpcListen: () => {} }));
vi.mock('@/hooks/queries/useUnprocessedStatements', () => ({
  useUnprocessedStatements: () => ({ data: groups }),
}));
vi.mock('@/hooks/mutations/useRetryUnprocessedStatement', () => ({
  useRetryUnprocessedStatement: () => ({ mutate: retryMutate, isPending: false }),
}));
vi.mock('@/hooks/mutations/useDiscardUnprocessedStatement', () => ({
  useDiscardUnprocessedStatement: () => ({ mutate: discardMutate, isPending: false }),
}));
vi.mock('@/hooks/mutations/useReparseAllStatements', () => ({
  useReparseAllStatements: () => ({ mutate: reparseMutate, isPending: false }),
}));

const entry = (over = {}) => ({
  statement_id: 's1',
  display_name: 'HDFCBANKXXXX1234JUN2026',
  filename: 'statement.pdf',
  failure_reason: null,
  ...over,
});

const renderQueue = () => render(<UnprocessedItemsQueue onEnterPassword={vi.fn()} />);

beforeEach(() => {
  vi.clearAllMocks();
  groups = { awaiting_password: [entry()], pending_retry: [], failed: [] };
});

describe('UnprocessedItemsQueue', () => {
  // Each group is a labelled section, so the aria-label is the stable handle.
  it('groups an item under its status section', () => {
    renderQueue();
    expect(screen.getByLabelText('Needs a password')).toBeInTheDocument();
    expect(screen.getByText('HDFCBANKXXXX1234JUN2026')).toBeInTheDocument();
  });

  it('hides groups that have no items', () => {
    renderQueue();
    expect(screen.queryByLabelText('Waiting to retry')).toBeNull();
    expect(screen.queryByLabelText("Couldn't be read")).toBeNull();
  });

  it('prefers the derived name and shows the filename underneath', () => {
    renderQueue();
    expect(screen.getByText('HDFCBANKXXXX1234JUN2026')).toBeInTheDocument();
    expect(screen.getByText('statement.pdf')).toBeInTheDocument();
  });

  it('falls back to the filename when the issuer could not be identified', () => {
    groups = { awaiting_password: [entry({ display_name: null })], pending_retry: [], failed: [] };
    renderQueue();
    expect(screen.getByText('statement.pdf')).toBeInTheDocument();
  });

  it('falls back again when there is no filename either', () => {
    groups = {
      awaiting_password: [entry({ display_name: null, filename: null })],
      pending_retry: [],
      failed: [],
    };
    renderQueue();
    expect(screen.getByText('Unknown file')).toBeInTheDocument();
  });

  it('shows the failure reason when one was recorded', () => {
    groups = {
      awaiting_password: [],
      pending_retry: [],
      failed: [entry({ failure_reason: 'no text layer' })],
    };
    renderQueue();
    expect(screen.getByText(/no text layer/)).toBeInTheDocument();
  });

  describe('re-parse all', () => {
    const runReparse = () => {
      renderQueue();
      fireEvent.click(screen.getByRole('button', { name: /Re-?parse/i }));
      return reparseMutate.mock.calls[0][1];
    };

    it('summarises a fully cleared queue', () => {
      const handlers = runReparse();
      handlers.onSuccess({ parsed: 3, total: 3, still_locked: 0, failed: 0 });
      expect(toast).toHaveBeenCalledWith({
        title: '3 of 3 parsed',
        description: 'The queue is clear.',
      });
    });

    it('counts locked and failed files as still needing attention', () => {
      const handlers = runReparse();
      handlers.onSuccess({ parsed: 1, total: 3, still_locked: 1, failed: 1 });
      expect(toast).toHaveBeenCalledWith(
        expect.objectContaining({
          title: '1 of 3 parsed',
          description: expect.stringContaining('2 still need attention'),
        })
      );
    });

    it('treats absent counters as zero', () => {
      const handlers = runReparse();
      handlers.onSuccess({ total: 2 });
      expect(toast).toHaveBeenCalledWith({
        title: '0 of 2 parsed',
        description: 'The queue is clear.',
      });
    });

    it('reports a re-parse failure destructively', () => {
      const handlers = runReparse();
      handlers.onError(new Error('sidecar crashed'));
      expect(toast).toHaveBeenCalledWith(
        expect.objectContaining({ variant: 'destructive', description: 'sidecar crashed' })
      );
    });
  });
});
