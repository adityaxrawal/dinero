import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import ClusterMemberComparison from '@/components/reconciliation/ClusterMemberComparison';
import type { ClusterMember } from '@/lib/ipc';

function makeMember(overrides: Partial<ClusterMember> = {}): ClusterMember {
  return {
    id: 'm1',
    member_role: 'candidate_a',
    observation_id: null,
    canonical_transaction_id: 'txn_1',
    source_pipeline: 'statement_pdf',
    merchant: 'Amazon',
    amount: 100,
    direction: 'debit',
    date: '2026-06-10',
    instrument_issuer_name: null,
    instrument_masked_identifier: null,
    reference_id: null,
    match_score: null,
    source_raw_payload_json: null,
    ...overrides,
  };
}

function renderWithRouter(ui: React.ReactElement) {
  return render(<MemoryRouter>{ui}</MemoryRouter>);
}

describe('ClusterMemberComparison', () => {
  it('shows instrument and reference ID when present', () => {
    renderWithRouter(
      <ClusterMemberComparison
        members={[
          makeMember({
            instrument_issuer_name: 'HDFC',
            instrument_masked_identifier: '4021',
            reference_id: 'REF123',
          }),
        ]}
      />
    );
    expect(screen.getByText('HDFC •• 4021')).toBeInTheDocument();
    expect(screen.getByText('REF123')).toBeInTheDocument();
  });

  it('omits the reference ID row entirely when absent', () => {
    renderWithRouter(<ClusterMemberComparison members={[makeMember({ reference_id: null })]} />);
    expect(screen.queryByText(/reference id/i)).not.toBeInTheDocument();
  });

  it('shows a score badge on candidate cards but not on the incoming card', () => {
    renderWithRouter(
      <ClusterMemberComparison
        members={[
          makeMember({
            member_role: 'incoming',
            match_score: null,
            canonical_transaction_id: null,
          }),
          makeMember({ member_role: 'candidate_a', match_score: 0.71 }),
        ]}
      />
    );
    expect(screen.getByText('71%')).toBeInTheDocument();
  });

  it('shows a "View full transaction" link on candidate cards', () => {
    renderWithRouter(
      <ClusterMemberComparison
        members={[makeMember({ member_role: 'candidate_a', canonical_transaction_id: 'txn_9' })]}
      />
    );
    const link = screen.getByRole('link', { name: /view full transaction/i });
    expect(link).toHaveAttribute('href', '/transactions/txn_9');
  });

  it('shows a source-message disclosure only on the incoming card', () => {
    renderWithRouter(
      <ClusterMemberComparison
        members={[
          makeMember({
            member_role: 'incoming',
            canonical_transaction_id: null,
            source_raw_payload_json: '{"body":"You spent Rs 100 at Amazon"}',
          }),
        ]}
      />
    );
    expect(screen.getByText(/view source message/i)).toBeInTheDocument();
  });

  it('labels a role it does not recognise with the raw role', () => {
    // ClusterMemberRole is a closed union covering every ROLE_LABEL key, so
    // this fallback only fires for a role the backend sends outside that
    // contract -- the cast is the point of the test, not a workaround.
    const rogue = makeMember({ member_role: 'candidate_a' });
    renderWithRouter(
      <ClusterMemberComparison members={[{ ...rogue, member_role: 'weird' as never }]} />
    );
    expect(screen.getByText('weird')).toBeInTheDocument();
  });

  it('omits the score bar when a candidate has no score', () => {
    renderWithRouter(<ClusterMemberComparison members={[makeMember({ match_score: null })]} />);
    expect(screen.queryByText(/%$/)).not.toBeInTheDocument();
  });

  it('omits the transaction link when the candidate is not yet canonicalised', () => {
    renderWithRouter(
      <ClusterMemberComparison members={[makeMember({ canonical_transaction_id: null })]} />
    );
    expect(screen.queryByRole('link', { name: /view full transaction/i })).not.toBeInTheDocument();
  });
});

// `onSelectCandidate` is what the cluster detail page's "Confirm Match" flow
// passes; the review queue renders the same card read-only. The two modes
// differ in every interactive attribute, so both are pinned here.
describe('ClusterMemberComparison candidate selection', () => {
  const selectable = (onSelect: (m: unknown) => void, selectedId: string | null = null) =>
    renderWithRouter(
      <ClusterMemberComparison
        members={[
          makeMember({ id: 'ref', member_role: 'incoming', canonical_transaction_id: null }),
          makeMember({ id: 'cand', member_role: 'candidate_a', canonical_transaction_id: 'txn_9' }),
        ]}
        selectedCandidateId={selectedId}
        onSelectCandidate={onSelect}
      />
    );

  it('exposes the candidate card as a button only when it can be picked', () => {
    const { unmount } = renderWithRouter(
      <ClusterMemberComparison members={[makeMember({ member_role: 'candidate_a' })]} />
    );
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    unmount();

    selectable(vi.fn());
    expect(screen.getByRole('button')).toHaveAttribute('tabindex', '0');
  });

  it('reports the picked candidate on click', () => {
    const onSelect = vi.fn();
    selectable(onSelect);
    fireEvent.click(screen.getByRole('button'));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onSelect.mock.calls[0][0]).toMatchObject({ id: 'cand' });
  });

  it.each(['Enter', ' '])('picks the candidate with the %s key', (key) => {
    const onSelect = vi.fn();
    selectable(onSelect);
    fireEvent.keyDown(screen.getByRole('button'), { key });
    expect(onSelect).toHaveBeenCalledTimes(1);
  });

  it('ignores other keys', () => {
    const onSelect = vi.fn();
    selectable(onSelect);
    fireEvent.keyDown(screen.getByRole('button'), { key: 'a' });
    fireEvent.keyDown(screen.getByRole('button'), { key: 'Tab' });
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('marks the currently selected candidate', () => {
    selectable(vi.fn(), 'txn_9');
    expect(screen.getByLabelText('Selected as match')).toBeInTheDocument();
  });

  it('marks nothing when the selection points elsewhere', () => {
    selectable(vi.fn(), 'txn_other');
    expect(screen.queryByLabelText('Selected as match')).not.toBeInTheDocument();
  });

  it('does not pick the candidate when following its transaction link', () => {
    // The link sits inside the clickable card; without stopPropagation,
    // navigating away would also silently confirm the match.
    const onSelect = vi.fn();
    selectable(onSelect);
    fireEvent.click(screen.getByRole('link', { name: /view full transaction/i }));
    expect(onSelect).not.toHaveBeenCalled();
  });
});
