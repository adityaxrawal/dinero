import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import ClusterMemberComparison from './ClusterMemberComparison';
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
    renderWithRouter(
      <ClusterMemberComparison members={[makeMember({ reference_id: null })]} />
    );
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
});
