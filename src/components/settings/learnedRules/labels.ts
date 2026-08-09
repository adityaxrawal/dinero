import type { LearnedRule } from '@/lib/ipc';

export const FIELD_LABELS: Record<string, string> = {
  merchant: 'the merchant name',
  amount: 'the amount',
  event_time: 'the transaction date',
  reference_id: 'the reference number',
  balance: 'the closing balance',
  last4: 'the card/account digits',
  direction: 'whether it is money in or out',
  currency: 'the currency',
};

/** Short chip form of a field name. */
export const FIELD_CHIPS: Record<string, string> = {
  merchant: 'merchant',
  amount: 'amount',
  event_time: 'date',
  reference_id: 'reference',
  balance: 'balance',
  last4: 'card digits',
  direction: 'in/out',
  currency: 'currency',
};

export const STATUS_STYLES: Record<string, string> = {
  trusted: 'bg-emerald-100 text-emerald-900 border-emerald-300',
  active: 'bg-[#F8E7C9] text-[#064E3B] border-[#064E3B]/20',
  pending: 'bg-amber-50 text-amber-900 border-amber-300',
  inactive: 'bg-neutral-100 text-neutral-600 border-neutral-300',
  flagged: 'bg-red-50 text-red-900 border-red-300',
};

export const STATUS_LABELS: Record<string, string> = {
  trusted: 'Proven',
  active: 'In use',
  pending: 'On trial',
  inactive: 'Retired',
  flagged: 'Flagged',
};

const LEARNED_FROM_LABELS: Record<string, string> = {
  user_edit: 'Learned from a correction you made',
  drift_llm: "Learned automatically after this bank's format changed",
  batch_cleanup: 'Learned during a merchant cleanup run',
};

/** Human-readable description of what a rule actually does. */
export function describeRule(rule: LearnedRule): string {
  const source = rule.source_type === 'email' ? 'email alerts' : 'PDF statements';
  const field = FIELD_LABELS[rule.field_name] ?? rule.field_name;

  if (rule.rule_payload_json.override_value !== undefined) {
    return `Always reads ${field} as "${rule.rule_payload_json.override_value}" for one ${rule.bank_name} ${source} format.`;
  }
  return `Reads ${field} out of ${rule.bank_name} ${source}.`;
}

/** Where the rule came from, in words. */
export function describeOrigin(rule: LearnedRule): string {
  const trigger = LEARNED_FROM_LABELS[rule.learned_from] ?? rule.learned_from;
  const author =
    rule.authored_by === 'llm' ? 'written by the on-device AI' : 'derived directly from the text';
  return `${trigger}, ${author}.`;
}

/** Track record in words rather than two bare counters. */
export function describeReliability(rule: LearnedRule): string {
  const worked = `worked ${rule.success_count} time${rule.success_count === 1 ? '' : 's'}`;
  return rule.failure_count > 0
    ? `${worked} · corrected again ${rule.failure_count}×`
    : `${worked} · never wrong`;
}

/** First 4 hex characters — enough to tell two formats apart at a glance. */
export function shortHash(hash: string): string {
  return (hash || '????').slice(0, 4).toUpperCase();
}

export function rulePattern(rule: LearnedRule): string {
  return rule.rule_payload_json.regex ?? rule.rule_payload_json.override_value ?? '';
}
