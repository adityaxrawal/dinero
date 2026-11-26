/**
 * User-facing wording for what the reconciliation engine did with a saved
 * transaction or statement. Commands used to report a bare "saved" even when
 * the engine matched the entry to an existing transaction or parked it for a
 * duplicate check (audit F9-5, F8-5, F12-4).
 */
import type { StatementCommitSummary } from '@/lib/ipc';
import { OUTCOME, inSentence, withCount } from '@/lib/outcomeVocabulary';

interface OutcomeToast {
  title: string;
  description: string;
  actionTo?: string;
  actionLabel?: string;
}

const CLUSTERS_ROUTE = '/reconciliation?section=clusters';

/** Toast for a single transaction, from the engine's decision string. */
export function engineDecisionToast(decision: string): OutcomeToast {
  if (decision === 'ambiguous_pending') {
    return {
      title: `Saved · ${OUTCOME.duplicateCheck}`,
      description: 'It looks like a transaction already in your ledger. Confirm it under Possible duplicates.',
      actionTo: CLUSTERS_ROUTE,
      actionLabel: 'Review',
    };
  }
  if (decision === 'auto_matched_exact' || decision === 'auto_matched_scored') {
    return {
      title: OUTCOME.matched,
      description: 'It was linked to a transaction already in your ledger, so nothing is counted twice.',
    };
  }
  return { title: OUTCOME.booked, description: 'Added to your ledger.' };
}

/** Toast for a committed statement, from its per-row summary. */
export function statementCommitToast(summary: StatementCommitSummary | undefined): OutcomeToast {
  if (!summary) return { title: 'Statement saved', description: 'Extracted transactions have been added.' };
  const parts = [
    summary.new > 0 && `${summary.new} ${inSentence(OUTCOME.booked)}`,
    summary.matched > 0 && `${summary.matched} ${inSentence(OUTCOME.matched)}`,
    summary.needs_review > 0 && withCount(summary.needs_review, OUTCOME.duplicateCheck),
    summary.skipped > 0 && `${summary.skipped} already imported`,
  ].filter(Boolean);
  return {
    title: 'Statement saved',
    description: parts.length > 0 ? `${parts.join(', ')}.` : 'No new transactions.',
    ...(summary.needs_review > 0 ? { actionTo: CLUSTERS_ROUTE, actionLabel: 'Review' } : {}),
  };
}
