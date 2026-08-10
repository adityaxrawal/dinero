import { describe, it, expect } from 'vitest';
import { diffClusterMember, summarizeClusterDiff } from '@/lib/clusterDiff';
import type { ClusterMember } from '@/lib/ipc';

function makeMember(overrides: Partial<ClusterMember> = {}): ClusterMember {
  return {
    id: 'm1',
    member_role: 'candidate_a',
    observation_id: null,
    canonical_transaction_id: 'txn_1',
    source_pipeline: 'statement_pdf',
    merchant: 'UPI_IKAS CORPORATE SOL',
    amount: 45,
    direction: 'debit',
    date: '2026-05-25',
    instrument_issuer_name: 'Yes Bank',
    instrument_masked_identifier: '2982',
    reference_id: null,
    match_score: 0.6,
    source_raw_payload_json: null,
    ...overrides,
  };
}

describe('diffClusterMember', () => {
  it('flags no diffs when every field matches the reference', () => {
    const reference = makeMember({ member_role: 'incoming' });
    const candidate = makeMember();
    expect(diffClusterMember(reference, candidate)).toEqual({
      merchant: false,
      instrument: false,
      amount: false,
      date: false,
    });
  });

  it('flags only the date when just the date differs', () => {
    const reference = makeMember({ member_role: 'incoming', date: '2026-05-26' });
    const candidate = makeMember({ date: '2026-05-25' });
    expect(diffClusterMember(reference, candidate)).toEqual({
      merchant: false,
      instrument: false,
      amount: false,
      date: true,
    });
  });

  it('treats a missing reference as no diff', () => {
    const candidate = makeMember();
    expect(diffClusterMember(undefined, candidate)).toEqual({
      merchant: false,
      instrument: false,
      amount: false,
      date: false,
    });
  });
});

describe('summarizeClusterDiff', () => {
  it('returns null when there is no reference or no candidates', () => {
    const reference = makeMember({ member_role: 'incoming' });
    expect(summarizeClusterDiff(undefined, [makeMember()])).toBeNull();
    expect(summarizeClusterDiff(reference, [])).toBeNull();
  });

  it('returns null when every candidate matches on every field', () => {
    const reference = makeMember({ member_role: 'incoming' });
    expect(summarizeClusterDiff(reference, [makeMember()])).toBeNull();
  });

  it('names the single differing field with both values when only one field differs', () => {
    const reference = makeMember({ member_role: 'incoming', date: '2026-05-26' });
    const candidateA = makeMember({ date: '2026-05-25' });
    const candidateB = makeMember({ date: '2026-05-27' });
    expect(summarizeClusterDiff(reference, [candidateA, candidateB])).toBe(
      "Only differs on: Date (25th May 26' vs 27th May 26')"
    );
  });

  it('lists multiple field names without a parenthetical when several fields differ', () => {
    const reference = makeMember({ member_role: 'incoming', date: '2026-05-26', amount: 45 });
    const candidateA = makeMember({ date: '2026-05-25', amount: 45 });
    const candidateB = makeMember({ date: '2026-05-27', amount: 99 });
    expect(summarizeClusterDiff(reference, [candidateA, candidateB])).toBe('Differs on: Amount, Date');
  });
});
