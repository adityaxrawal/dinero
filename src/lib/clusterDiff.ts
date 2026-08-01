import type { ClusterMember } from '@/lib/ipc';
import { formatCustomDate } from '@/lib/formatCustomDate';

export interface ClusterMemberDiff {
  merchant: boolean;
  instrument: boolean;
  amount: boolean;
  date: boolean;
}

const DIFF_FIELDS = ['merchant', 'instrument', 'amount', 'date'] as const;

const FIELD_LABELS: Record<(typeof DIFF_FIELDS)[number], string> = {
  merchant: 'Merchant',
  instrument: 'Instrument',
  amount: 'Amount',
  date: 'Date',
};

/** Per-field diff of a candidate against the "incoming" reference member. */
export function diffClusterMember(
  reference: ClusterMember | undefined,
  candidate: ClusterMember
): ClusterMemberDiff {
  return {
    merchant: !!reference && candidate.merchant !== reference.merchant,
    instrument:
      !!reference &&
      (candidate.instrument_issuer_name !== reference.instrument_issuer_name ||
        candidate.instrument_masked_identifier !== reference.instrument_masked_identifier),
    amount: !!reference && candidate.amount !== reference.amount,
    date: !!reference && candidate.date !== reference.date,
  };
}

function formatFieldValue(field: (typeof DIFF_FIELDS)[number], member: ClusterMember): string {
  switch (field) {
    case 'merchant':
      return member.merchant;
    case 'instrument':
      return (
        [member.instrument_issuer_name, member.instrument_masked_identifier]
          .filter(Boolean)
          .join(' •• ') || 'Unknown'
      );
    case 'amount':
      return `₹${Math.abs(member.amount).toFixed(2)}`;
    case 'date':
      return member.date === 'Unknown' ? 'Unknown' : formatCustomDate(member.date);
  }
}

/**
 * One-line verdict naming exactly which field(s) distinguish the candidates
 * from the incoming evidence -- e.g. "Only differs on: Date (25th May 26' vs
 * 27th May 26')". Returns null when there's nothing to compare (no reference,
 * no candidates, or every field matches), so callers can fall back to the
 * cluster's own server-computed explanation.
 */
export function summarizeClusterDiff(
  reference: ClusterMember | undefined,
  candidates: ClusterMember[]
): string | null {
  if (!reference || candidates.length === 0) return null;

  const diffs = candidates.map((c) => diffClusterMember(reference, c));
  const differingFields = DIFF_FIELDS.filter((f) => diffs.some((d) => d[f]));
  if (differingFields.length === 0) return null;

  const label = differingFields.length === 1 ? 'Only differs on' : 'Differs on';
  const names = differingFields.map((f) => FIELD_LABELS[f]).join(', ');

  if (differingFields.length === 1) {
    const field = differingFields[0];
    const uniqueValues = Array.from(new Set(candidates.map((c) => formatFieldValue(field, c))));
    if (uniqueValues.length > 1) {
      return `${label}: ${names} (${uniqueValues.join(' vs ')})`;
    }
  }

  return `${label}: ${names}`;
}
