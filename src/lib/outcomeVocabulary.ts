/**
 * The one set of words every screen uses for what happened to an email,
 * statement row or transaction (audit Phase 2 §2.1). Before this, the same
 * state read "Ignored", "Action Needed", "Action Required", "Missing Fields",
 * "Pending Review" or "Unresolved Clusters" depending on the screen.
 *
 * Backend text that the user sees (Settings > Recent activity) uses the same
 * words; keep src-tauri/src/ingestion/historical_scan.rs in step.
 */

export const OUTCOME = {
  /** Became a transaction in the ledger. */
  booked: 'Booked',
  /** Linked to a transaction already in the ledger, so nothing is counted twice. */
  matched: 'Matched to existing',
  /** Looks like a transaction already in the ledger; the user decides. */
  duplicateCheck: 'Needs duplicate check',
  /** Missing details the user fills in (Review Inbox). */
  needsReview: 'Needs review',
  /** Held for the local AI to read. */
  waitingForAi: 'Waiting for AI',
  /** Blocked on the user: a statement password or account details. */
  waitingForYou: 'Waiting for you',
  /** Not a transaction (OTP, marketing, notice) or from a sender not trusted. */
  skipped: 'Skipped',
  /** Could not be read; can be retried. */
  failed: 'Failed',
} as const;

/** A term for use mid-sentence: first letter lowered, so "AI" stays "AI". */
export function inSentence(term: string): string {
  return term.charAt(0).toLowerCase() + term.slice(1);
}

/** Counters a scan reports, as sent in the scan events. */
export interface ScanOutcomeCounts {
  processed: number;
  transactions_found: number;
  statements_found: number;
  sent_to_review?: number;
  pending_enrichment?: number;
  non_financial?: number;
  errors: number;
}

/**
 * One sentence describing a finished scan, used by the notification feed and
 * onboarding. Zero counts are left out so the sentence stays short.
 */
export function scanSummary(p: ScanOutcomeCounts): string {
  const n = (v: number | undefined) => v ?? 0;
  const parts = [
    `${n(p.transactions_found)} ${inSentence(OUTCOME.booked)}`,
    n(p.sent_to_review) > 0 && `${n(p.sent_to_review)} ${inSentence(OUTCOME.needsReview)}`,
    n(p.pending_enrichment) > 0 &&
      `${n(p.pending_enrichment)} ${inSentence(OUTCOME.waitingForAi)}`,
    n(p.statements_found) > 0 &&
      `${n(p.statements_found)} statement${n(p.statements_found) === 1 ? '' : 's'}`,
    n(p.non_financial) > 0 && `${n(p.non_financial)} ${inSentence(OUTCOME.skipped)}`,
    n(p.errors) > 0 && `${n(p.errors)} ${inSentence(OUTCOME.failed)}`,
  ].filter(Boolean);
  return `Checked ${n(p.processed)} emails: ${parts.join(', ')}.`;
}

/** Short label for why a review entry needs the user (list badges). */
const REVIEW_REASON_LABEL: Record<string, string> = {
  extraction_failed: 'Missing details',
  issuer_name_not_found: 'Unknown card or bank',
  pending_llm_enrichment: OUTCOME.waitingForAi,
  'gate3_failed:missing_amount': 'Missing amount',
  'gate3_failed:missing_counterparty': 'Missing merchant',
  'gate3_failed:missing_instrument': 'Missing card or account',
  'gate3_failed:missing_direction': 'Missing debit/credit',
  'gate3_failed:missing_currency': 'Missing currency',
  'gate3_failed:low_confidence': 'Check details',
  llm_value_not_in_email: 'Not in the email',
  instrument_archived: 'Archived card',
  unverified_sender: 'Unverified sender',
};

export function reviewReasonLabel(reason: string): string {
  return REVIEW_REASON_LABEL[reason] ?? OUTCOME.needsReview;
}
