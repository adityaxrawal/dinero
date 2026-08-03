import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useResolveClusterActions } from './useResolveClusterActions';
import type { ClusterMember, ClusterRecord } from '@/lib/ipc';
import { API } from '@/lib/ipc';

const toast = vi.fn();
const mutate = vi.fn();
const ask = vi.fn();

vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/hooks/mutations/useResolveCluster', () => ({
  useResolveCluster: () => ({ mutate, isPending: false }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ ask: (...args: unknown[]) => ask(...args) }));
vi.mock('@/lib/ipc', () => ({
  API: { reconciliation: { unmergeCluster: vi.fn().mockResolvedValue(undefined) } },
}));
vi.mock('@/components/ui/toast', () => ({
  ToastAction: ({ children, onClick }: { children: React.ReactNode; onClick: () => void }) => (
    <button onClick={onClick}>{children}</button>
  ),
}));

const member = (over: Partial<ClusterMember> = {}): ClusterMember => ({
  id: 'm1',
  member_role: 'candidate_a',
  observation_id: null,
  canonical_transaction_id: 'canon1',
  source_pipeline: 'statement',
  merchant: 'Swiggy',
  amount: 450.5,
  direction: 'debit',
  date: '2026-01-01',
  instrument_issuer_name: 'HDFC',
  instrument_masked_identifier: '8841',
  reference_id: null,
  match_score: 0.9,
  source_raw_payload_json: null,
  ...over,
});

const cluster = (over: Partial<ClusterRecord> = {}): ClusterRecord => ({
  id: 'c1',
  reason: 'amount_date',
  members_count: 2,
  members: [member({ id: 'in', member_role: 'incoming', observation_id: 'obs1' }), member()],
  created_at: '2026-01-01',
  explanation: 'looks similar',
  ...over,
});

const setup = (record: ClusterRecord | undefined = cluster(), onSuccess = vi.fn()) => {
  const hook = renderHook(() => useResolveClusterActions({ cluster: record, onSuccess }));
  return { ...hook, onSuccess };
};

beforeEach(() => {
  vi.clearAllMocks();
  ask.mockResolvedValue(true);
});

describe('useResolveClusterActions', () => {
  it('separates the incoming member from the candidates', () => {
    const { result } = setup();
    expect(result.current.candidates).toHaveLength(1);
    expect(result.current.candidates[0].member_role).toBe('candidate_a');
  });

  it('returns no candidates for an undefined cluster', () => {
    const { result } = renderHook(() =>
      useResolveClusterActions({ cluster: undefined, onSuccess: vi.fn() })
    );
    expect(result.current.candidates).toEqual([]);
  });

  it('clears the selection when the cluster changes', () => {
    const { result, rerender } = renderHook(
      ({ c }) => useResolveClusterActions({ cluster: c, onSuccess: vi.fn() }),
      { initialProps: { c: cluster() } }
    );
    act(() => result.current.setSelectedCandidateId('canon1'));
    expect(result.current.selectedCandidateId).toBe('canon1');
    rerender({ c: cluster({ id: 'c2' }) });
    expect(result.current.selectedCandidateId).toBeNull();
  });

  it('refuses to confirm without a selected candidate', async () => {
    const { result } = setup();
    await act(async () => result.current.handleConfirmMatch());
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Pick a match first' }));
    expect(mutate).not.toHaveBeenCalled();
  });

  it('refuses to act on a cluster with no incoming evidence', async () => {
    const { result } = setup(cluster({ members: [member()] }));
    await act(async () => result.current.handleKeepSeparate());
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Cannot resolve' }));
    expect(mutate).not.toHaveBeenCalled();
  });

  it('confirms a match with the chosen canonical id', async () => {
    const { result } = setup();
    act(() => result.current.setSelectedCandidateId('canon1'));
    await act(async () => result.current.handleConfirmMatch());
    await waitFor(() =>
      expect(mutate).toHaveBeenCalledWith(
        expect.objectContaining({
          clusterId: 'c1',
          observationId: 'obs1',
          action: 'confirm_match',
          chosenCanonicalId: 'canon1',
        }),
        expect.anything()
      )
    );
  });

  it('shows the merchant and amount in the confirmation prompt', async () => {
    const { result } = setup();
    act(() => result.current.setSelectedCandidateId('canon1'));
    await act(async () => result.current.handleConfirmMatch());
    expect(ask).toHaveBeenCalledWith(
      expect.stringContaining('Swiggy'),
      expect.objectContaining({ title: 'Confirm Match', kind: 'warning' })
    );
    expect(ask.mock.calls[0][0]).toContain('450.50');
  });

  it('aborts when the user declines the dialog', async () => {
    ask.mockResolvedValue(false);
    const { result } = setup();
    await act(async () => result.current.handleKeepSeparate());
    expect(mutate).not.toHaveBeenCalled();
  });

  it('falls back to window.confirm when the Tauri dialog is unavailable', async () => {
    ask.mockRejectedValue(new Error('not in tauri'));
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    const { result } = setup();
    await act(async () => result.current.handleKeepSeparate());
    expect(confirmSpy).toHaveBeenCalled();
    await waitFor(() => expect(mutate).toHaveBeenCalled());
    confirmSpy.mockRestore();
  });

  it.each([
    ['handleRejectCandidates', 'reject_candidate'],
    ['handleKeepSeparate', 'keep_separate'],
    ['handleReviewLater', 'mark_unresolved'],
  ] as const)('%s sends the %s action', async (method, action) => {
    const { result } = setup();
    await act(async () => result.current[method]());
    await waitFor(() =>
      expect(mutate).toHaveBeenCalledWith(expect.objectContaining({ action }), expect.anything())
    );
  });

  describe('post-resolution feedback', () => {
    const runSuccess = async (method: 'handleConfirmMatch' | 'handleReviewLater' | 'handleKeepSeparate') => {
      const { result, onSuccess } = setup();
      if (method === 'handleConfirmMatch') act(() => result.current.setSelectedCandidateId('canon1'));
      await act(async () => result.current[method]());
      await waitFor(() => expect(mutate).toHaveBeenCalled());
      act(() => mutate.mock.calls[0][1].onSuccess());
      return onSuccess;
    };

    it('tells the user a review-later cluster stays queued', async () => {
      await runSuccess('handleReviewLater');
      expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Marked for Later Review' }));
    });

    it('offers an Undo action after a confirmed match', async () => {
      await runSuccess('handleConfirmMatch');
      const call = toast.mock.calls.find((c) => c[0].title === 'Cluster Resolved');
      expect(call![0].action).toBeTruthy();
    });

    it('offers no Undo for a keep-separate decision', async () => {
      await runSuccess('handleKeepSeparate');
      const call = toast.mock.calls.find((c) => c[0].title === 'Cluster Resolved');
      expect(call![0].action).toBeUndefined();
    });

    it('notifies the caller so it can close the panel', async () => {
      const onSuccess = await runSuccess('handleKeepSeparate');
      expect(onSuccess).toHaveBeenCalled();
    });

    it('unmerges the snapshotted cluster id when Undo is pressed', async () => {
      const { result } = setup();
      act(() => result.current.setSelectedCandidateId('canon1'));
      await act(async () => result.current.handleConfirmMatch());
      await waitFor(() => expect(mutate).toHaveBeenCalled());
      act(() => mutate.mock.calls[0][1].onSuccess());
      const action = toast.mock.calls.find((c) => c[0].title === 'Cluster Resolved')![0].action;
      await act(async () => action.props.onClick());
      expect(API.reconciliation.unmergeCluster).toHaveBeenCalledWith('c1');
    });

    it('surfaces a destructive toast when the mutation fails', async () => {
      const { result } = setup();
      await act(async () => result.current.handleKeepSeparate());
      await waitFor(() => expect(mutate).toHaveBeenCalled());
      act(() => mutate.mock.calls[0][1].onError(new Error('db locked')));
      expect(toast).toHaveBeenCalledWith(
        expect.objectContaining({ variant: 'destructive', description: 'db locked' })
      );
    });
  });
});
