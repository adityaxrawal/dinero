// One set of outcome words across screens (audit Phase 2 §2.1).
import { describe, it, expect } from 'vitest';
import { OUTCOME, reviewReasonLabel, scanSummary, withCount } from '@/lib/outcomeVocabulary';

describe('outcome vocabulary', () => {
  it('summarises a scan with the shared words, leaving out zero counts', () => {
    expect(
      scanSummary({
        processed: 258,
        transactions_found: 34,
        statements_found: 11,
        sent_to_review: 8,
        pending_enrichment: 50,
        non_financial: 153,
        errors: 0,
      })
    ).toBe('Checked 258 emails: 34 booked, 8 need review, 50 waiting for AI, 11 statements, 153 skipped.');
  });

  it('labels review reasons, falling back to Needs review', () => {
    expect(reviewReasonLabel('gate3_failed:missing_instrument')).toBe('Missing card or account');
    expect(reviewReasonLabel('pending_llm_enrichment')).toBe(OUTCOME.waitingForAi);
    expect(reviewReasonLabel('something_new')).toBe(OUTCOME.needsReview);
  });
});

describe('withCount', () => {
  it('agrees in number for the Needs terms only', () => {
    expect(withCount(1, OUTCOME.needsReview)).toBe('1 needs review');
    expect(withCount(8, OUTCOME.needsReview)).toBe('8 need review');
    expect(withCount(2, OUTCOME.duplicateCheck)).toBe('2 need duplicate check');
    expect(withCount(5, OUTCOME.booked)).toBe('5 booked');
  });
});
