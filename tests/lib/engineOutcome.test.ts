// Saves used to say "saved" whatever the engine decided (audit F9-5, F8-5).
import { describe, it, expect } from 'vitest';
import { engineDecisionToast, statementCommitToast } from '@/lib/engineOutcome';

describe('engineDecisionToast', () => {
  it('sends a parked duplicate to Pending Clusters', () => {
    const t = engineDecisionToast('ambiguous_pending');
    expect(t.title).toContain('duplicate check');
    expect(t.actionTo).toBe('/reconciliation?section=clusters');
  });

  it('says when the entry matched an existing transaction', () => {
    expect(engineDecisionToast('auto_matched_scored').title).toBe('Matched to existing');
    expect(engineDecisionToast('auto_matched_exact').title).toBe('Matched to existing');
  });

  it('falls back to a plain saved message', () => {
    expect(engineDecisionToast('new_canonical').title).toBe('Booked');
  });
});

describe('statementCommitToast', () => {
  it('lists each outcome and links to the duplicate check', () => {
    const t = statementCommitToast({ new: 12, matched: 3, needs_review: 1, skipped: 2 });
    expect(t.description).toBe(
      '12 booked, 3 matched to existing, 1 needs duplicate check, 2 already imported.'
    );
    expect(t.actionTo).toBe('/reconciliation?section=clusters');
  });

  it('has no link when nothing needs review', () => {
    expect(statementCommitToast({ new: 1, matched: 0, needs_review: 0, skipped: 0 }).actionTo).toBeUndefined();
  });
});
