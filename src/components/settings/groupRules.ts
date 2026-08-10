/**
 * Groups learned rules by bank for display.
 *
 * Pure and separate from the component so the grouping is directly testable.
 */
import type { LearnedRule } from '@/lib/ipc';

export type FormatGroup = {
  templateHash: string;
  rules: LearnedRule[];
  confidence: number;
  fields: string[];
  learnedAt: string | null;
};

export type BankGroup = {
  bank: string;
  formats: FormatGroup[];
  ruleCount: number;
  confidence: number;
};

/** Groups learned rules by bank for display. */
export function groupRules(rules: LearnedRule[]): BankGroup[] {
  const banks = new Map<string, Map<string, LearnedRule[]>>();
  for (const rule of rules) {
    const formats = banks.get(rule.bank_name) ?? new Map<string, LearnedRule[]>();
    banks.set(rule.bank_name, formats);
    const bucket = formats.get(rule.template_hash) ?? [];
    bucket.push(rule);
    formats.set(rule.template_hash, bucket);
  }

  return [...banks.entries()]
    .map(([bank, formats]) => {
      const groups: FormatGroup[] = [...formats.entries()]
        .map(([templateHash, groupRules]) => ({
          templateHash,
          rules: groupRules,
          confidence: Math.min(...groupRules.map((r) => r.confidence)),
          fields: [...new Set(groupRules.map((r) => r.field_name))],
          learnedAt:
            groupRules
              .map((r) => r.created_at)
              .filter((d): d is string => !!d)
              .sort()
              .at(-1) ?? null,
        }))
        .sort((a, b) => (b.learnedAt ?? '').localeCompare(a.learnedAt ?? ''));

      const all = groups.flatMap((g) => g.rules);
      return {
        bank,
        formats: groups,
        ruleCount: all.length,
        confidence: all.reduce((sum, r) => sum + r.confidence, 0) / all.length,
      };
    })
    .sort((a, b) => b.ruleCount - a.ruleCount || a.bank.localeCompare(b.bank));
}
