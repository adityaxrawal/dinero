import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ConfidenceMeter, RelativeDate } from './SettingsPrimitives';
import { groupRules } from './LearnedRulesSettings';
import type { LearnedRule } from '@/lib/ipc';

/**
 * The band boundaries are the whole point of the component: they decide whether
 * a rule reads as trustworthy, so an off-by-one at 0.70 or 0.90 silently
 * mislabels rules the user is being asked to judge.
 */
describe('ConfidenceMeter', () => {
  const cases: [number, string][] = [
    [0.98, 'Reliable'],
    [0.9, 'Reliable'],
    [0.89, 'Holding up'],
    [0.7, 'Holding up'],
    [0.69, 'Unproven'],
    [0.5, 'Unproven'],
    [0.49, 'Weak'],
    [0, 'Weak'],
  ];

  for (const [value, label] of cases) {
    it(`reads "${label}" at ${value}`, () => {
      render(<ConfidenceMeter value={value} />);
      expect(screen.getByText(label)).toBeInTheDocument();
    });
  }

  it('keeps the exact figure available on hover', () => {
    const { container } = render(<ConfidenceMeter value={0.83} />);
    expect(container.querySelector('[title="83% confidence"]')).not.toBeNull();
  });
});

describe('RelativeDate', () => {
  it('uses relative wording inside a week', () => {
    const twoHoursAgo = new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString();
    render(<RelativeDate iso={twoHoursAgo} />);
    expect(screen.getByText(/hours ago/)).toBeInTheDocument();
  });

  it('switches to a calendar date past a week', () => {
    const longAgo = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString();
    render(<RelativeDate iso={longAgo} />);
    expect(screen.queryByText(/ago/)).toBeNull();
  });

  /**
   * SQLite writes `CURRENT_TIMESTAMP` as "YYYY-MM-DD HH:MM:SS" with no zone
   * marker. Handing that straight to `new Date()` is invalid in Safari, which is
   * the engine this app actually runs on.
   */
  it('parses SQLite timestamps rather than rendering "unknown date"', () => {
    render(<RelativeDate iso="2020-01-01 00:00:00" />);
    expect(screen.queryByText('unknown date')).toBeNull();
  });

  it('says so when there is no date at all', () => {
    render(<RelativeDate iso={null} />);
    expect(screen.getByText('unknown date')).toBeInTheDocument();
  });
});

function rule(overrides: Partial<LearnedRule>): LearnedRule {
  return {
    id: Math.random().toString(36).slice(2),
    bank_name: 'Yes Bank',
    field_name: 'merchant',
    source_type: 'email',
    template_hash: 'a3f9deadbeef',
    rule_payload_json: { regex: 'PYU\\*(.+)', capture_group: 1 },
    status: 'active',
    success_count: 1,
    failure_count: 0,
    confidence: 0.8,
    authored_by: 'llm',
    learned_from: 'batch_cleanup',
    created_at: '2026-07-01 10:00:00',
    updated_at: '2026-07-01 10:00:00',
    ...overrides,
  };
}

/**
 * The defect this grouping exists to fix: six rules for one bank all describe
 * themselves with the same sentence, so the old flat list looked like duplicated
 * rows. They are distinguished by `template_hash` — which layout of that bank's
 * mail they read — and must never be merged.
 */
describe('groupRules', () => {
  it('keeps rules with different template hashes as separate formats', () => {
    const groups = groupRules([
      rule({ template_hash: 'aaaa1111' }),
      rule({ template_hash: 'bbbb2222' }),
      rule({ template_hash: 'cccc3333' }),
    ]);

    expect(groups).toHaveLength(1);
    expect(groups[0].bank).toBe('Yes Bank');
    expect(groups[0].formats).toHaveLength(3);
    expect(groups[0].ruleCount).toBe(3);
  });

  it('merges rules that really do describe the same email format', () => {
    const groups = groupRules([
      rule({ template_hash: 'aaaa1111', field_name: 'merchant' }),
      rule({ template_hash: 'aaaa1111', field_name: 'amount' }),
    ]);

    expect(groups[0].formats).toHaveLength(1);
    expect(groups[0].formats[0].fields).toEqual(['merchant', 'amount']);
  });

  it('splits by bank and leads with the bank holding the most rules', () => {
    const groups = groupRules([
      rule({ bank_name: 'HDFC Bank', template_hash: 'h1' }),
      rule({ bank_name: 'Yes Bank', template_hash: 'y1' }),
      rule({ bank_name: 'Yes Bank', template_hash: 'y2' }),
    ]);

    expect(groups.map((g) => g.bank)).toEqual(['Yes Bank', 'HDFC Bank']);
  });

  /** A format's headline confidence must be its weakest rule, not its average. */
  it('reports the weakest rule in a format', () => {
    const groups = groupRules([
      rule({ template_hash: 'same', confidence: 0.95 }),
      rule({ template_hash: 'same', confidence: 0.6 }),
    ]);

    expect(groups[0].formats[0].confidence).toBeCloseTo(0.6);
  });
});
