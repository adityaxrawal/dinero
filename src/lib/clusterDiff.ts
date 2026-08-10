/**
 * Explains why the transactions in a reconciliation cluster are not identical.
 *
 * A cluster groups transactions the backend believes may be the same real-world
 * payment seen through different sources. Before merging them the user needs to
 * know exactly where they disagree -- two charges matching on merchant, amount
 * and instrument but differing only by date are almost certainly one event,
 * whereas differing amounts suggest genuinely separate transactions.
 *
 * The comparison is always against a single reference member, so the output
 * reads as "how the others differ from this one" rather than an unanchored
 * pairwise matrix.
 */
import type { ClusterMember } from '@/lib/ipc';
import { formatCustomDate } from '@/lib/formatCustomDate';

/** Which fields differ; one boolean per comparable field. */
export interface ClusterMemberDiff {
  merchant: boolean;
  instrument: boolean;
  amount: boolean;
  date: boolean;
}

// Declaration order doubles as display order in the generated summary.
const DIFF_FIELDS = ['merchant', 'instrument', 'amount', 'date'] as const;

const FIELD_LABELS: Record<(typeof DIFF_FIELDS)[number], string> = {
  merchant: 'Merchant',
  instrument: 'Instrument',
  amount: 'Amount',
  date: 'Date',
};

/**
 * Compare one candidate against the reference member, field by field.
 *
 * With no reference every field reports false rather than true: absent a
 * baseline there is nothing to differ *from*, and defaulting to "everything
 * differs" would paint the entire row as conflicting.
 */
export function diffClusterMember(
  reference: ClusterMember | undefined,
  candidate: ClusterMember
): ClusterMemberDiff {
  return {
    merchant: !!reference && candidate.merchant !== reference.merchant,
    // Instrument identity is the issuer and the masked account number together;
    // either one changing means a different payment instrument.
    instrument:
      !!reference &&
      (candidate.instrument_issuer_name !== reference.instrument_issuer_name ||
        candidate.instrument_masked_identifier !== reference.instrument_masked_identifier),
    amount: !!reference && candidate.amount !== reference.amount,
    date: !!reference && candidate.date !== reference.date,
  };
}

/** Render one field of a member as display text for the summary line. */
function formatFieldValue(field: (typeof DIFF_FIELDS)[number], member: ClusterMember): string {
  switch (field) {
    case 'merchant':
      return member.merchant;
    case 'instrument':
      // Either part may be missing, so empties are dropped before joining and
      // a member with neither falls back to a readable placeholder.
      return (
        [member.instrument_issuer_name, member.instrument_masked_identifier]
          .filter(Boolean)
          .join(' •• ') || 'Unknown'
      );
    case 'amount':
      // Absolute value: direction is conveyed elsewhere in the row, and a sign
      // here would read as part of the number being compared.
      return `₹${Math.abs(member.amount).toFixed(2)}`;
    case 'date':
      // 'Unknown' is a real sentinel from extraction, not a parseable date, so
      // it is passed through rather than fed to the formatter.
      return member.date === 'Unknown' ? 'Unknown' : formatCustomDate(member.date);
  }
}

/**
 * Produce a one-line explanation of how a cluster's members disagree.
 *
 * Returns null when there is nothing useful to say -- no reference, no
 * candidates, or every field matching -- so the caller can omit the line
 * entirely instead of rendering an empty or reassuring-but-meaningless string.
 */
export function summarizeClusterDiff(
  reference: ClusterMember | undefined,
  candidates: ClusterMember[]
): string | null {
  if (!reference || candidates.length === 0) return null;

  // A field is reported if *any* candidate differs on it, so the summary
  // describes the cluster as a whole rather than one pair within it.
  const diffs = candidates.map((c) => diffClusterMember(reference, c));
  const differingFields = DIFF_FIELDS.filter((f) => diffs.some((d) => d[f]));
  if (differingFields.length === 0) return null;

  // "Only differs on" is a deliberately reassuring phrasing for the single-field
  // case -- one difference is the strongest signal that these are the same
  // payment and safe to merge.
  const label = differingFields.length === 1 ? 'Only differs on' : 'Differs on';
  const names = differingFields.map((f) => FIELD_LABELS[f]).join(', ');

  // When exactly one field differs, the actual values are cheap to show and
  // let the user decide without opening the cluster. Values are deduplicated
  // because many candidates commonly share the same differing value.
  if (differingFields.length === 1) {
    const field = differingFields[0];
    const uniqueValues = Array.from(new Set(candidates.map((c) => formatFieldValue(field, c))));
    if (uniqueValues.length > 1) {
      return `${label}: ${names} (${uniqueValues.join(' vs ')})`;
    }
  }

  // Several differing fields: naming them is enough, since inlining every value
  // would produce a line too dense to scan.
  return `${label}: ${names}`;
}
