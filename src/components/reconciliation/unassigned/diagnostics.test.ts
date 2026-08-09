// The branchy derivations pulled out of the old 502-line UnassignedInspector:
// what to call a failure, how to describe it, how far to trust the extraction,
// and which gate dropped the message.
import { describe, it, expect } from 'vitest';
import type { UnassignedTransactionRecord } from '@/lib/ipc';
import {
  reasonTitle,
  reasonGuidance,
  extractionBadge,
  buildChecks,
  parseEmailEvidence,
} from './diagnostics';

const record = (over: Partial<UnassignedTransactionRecord> = {}): UnassignedTransactionRecord =>
  ({
    id: 'u1',
    observation_id: 'obs1',
    reason: 'extraction_failed',
    status: 'open',
    created_at: null,
    merchant_raw: 'Google Cloud',
    amount_minor: 3152,
    currency: 'INR',
    direction: null,
    event_time: '2026-07-29 10:00:00',
    source_message_id: null,
    body_snippet: 'You spent Rs 31.52',
    raw_payload_json: null,
    ...over,
  }) as UnassignedTransactionRecord;

const checkFor = (rec: UnassignedTransactionRecord, id: string) =>
  buildChecks(rec).find((c) => c.id === id)!;

describe('reasonTitle', () => {
  it('names each known failure reason', () => {
    expect(reasonTitle('extraction_failed')).toBe('Failed to Extract Details');
    expect(reasonTitle('gate3_failed:missing_amount')).toBe('Missing Transaction Amount');
  });

  it('falls back to the raw reason, then to a generic label', () => {
    expect(reasonTitle('something_new')).toBe('something_new');
    expect(reasonTitle('')).toBe('Unresolved Issue');
  });
});

describe('reasonGuidance', () => {
  it('explains the failure and says what to do about it', () => {
    const { description, tip } = reasonGuidance('issuer_name_not_found');
    expect(description).toMatch(/could not match the bank or card/);
    expect(tip).toMatch(/Select an existing instrument/);
  });

  it('falls back to generic guidance for an unrecognised reason', () => {
    expect(reasonGuidance('gate3_failed').description).toMatch(/unexpected issue/);
    expect(reasonGuidance('brand_new_reason').tip).toMatch(/Review the extracted data/);
  });
});

describe('extractionBadge', () => {
  it('treats a regex match as fully deterministic', () => {
    const badge = extractionBadge(record({ extraction_method: 'regex_hdfc_card' }));
    expect(badge.engineLabel).toBe('Deterministic (regex_hdfc_card)');
    expect(badge.confidenceLabel).toBe('100% (Rule Match)');
    expect(badge.confidenceBadgeStyle).toMatch(/emerald/);
  });

  it('reports the LLM score and colours it by band', () => {
    const at = (score: number) =>
      extractionBadge(record({ extraction_method: 'llm_layer6', confidence_score: score }));
    expect(at(0.92).confidenceLabel).toBe('92% Confidence');
    expect(at(0.92).confidenceBadgeStyle).toMatch(/emerald/);
    expect(at(0.6).confidenceBadgeStyle).toMatch(/amber/);
    // Below 50% the model's answer is no better than no answer, so it stays red.
    expect(at(0.3).confidenceBadgeStyle).toMatch(/red/);
  });

  it('calls an unscored LLM extraction medium rather than inventing a number', () => {
    const badge = extractionBadge(record({ extraction_method: 'llm_layer6' }));
    expect(badge.confidenceLabel).toBe('AI Extracted (Medium)');
    expect(badge.engineLabel).toBe('AI Layer 6 LLM');
  });

  it('marks a queued enrichment as pending, not failed', () => {
    const badge = extractionBadge(record({ reason: 'pending_llm_enrichment' }));
    expect(badge.engineLabel).toBe('AI Layer 6 LLM (Pending)');
    expect(badge.confidenceLabel).toBe('Enqueued for AI');
  });

  it('scores a plain ladder extraction on a two-band scale', () => {
    expect(extractionBadge(record({ confidence_score: 0.85 })).confidenceBadgeStyle).toMatch(
      /emerald/
    );
    expect(extractionBadge(record({ confidence_score: 0.2 })).confidenceBadgeStyle).toMatch(/amber/);
  });

  it('reports nothing extracted when there is no method and no score', () => {
    const badge = extractionBadge(record());
    expect(badge.engineLabel).toBe('Standard Ladder');
    expect(badge.confidenceLabel).toBe('0% (Unextracted)');
    expect(badge.confidenceBadgeStyle).toMatch(/red/);
  });
});

describe('buildChecks', () => {
  it('passes the two gates that always ran, and formats a present amount', () => {
    const checks = buildChecks(record());
    expect(checks.filter((c) => c.passed).map((c) => c.id)).toContain('gate1');
    expect(checkFor(record(), 'amount').value).toBe('INR 31.52');
  });

  it('fails the fields that are actually missing', () => {
    const bare = record({ amount_minor: null, merchant_raw: null, event_time: null });
    expect(checkFor(bare, 'amount')).toMatchObject({ passed: false, value: 'Missing / Unparsed' });
    expect(checkFor(bare, 'merchant').passed).toBe(false);
    expect(checkFor(bare, 'date').passed).toBe(false);
  });

  it('distinguishes an unmatched instrument from an absent one', () => {
    expect(checkFor(record({ reason: 'issuer_name_not_found' }), 'instrument')).toMatchObject({
      passed: false,
      value: 'Card/Account Unmatched in Settings',
    });
    expect(
      checkFor(record({ reason: 'gate3_failed:missing_instrument' }), 'instrument')
    ).toMatchObject({ passed: false, value: 'No Card/Account Identifier in Email' });
    expect(checkFor(record(), 'instrument').passed).toBe(true);
  });
});

describe('parseEmailEvidence', () => {
  it('reads html, body, subject and sender out of the stored payload', () => {
    const parsed = parseEmailEvidence(
      record({
        raw_payload_json: JSON.stringify({
          html: '<p>hi</p>',
          body: 'hi',
          subject: 'Alert',
          sender: 'bank@x.com',
        }),
      })
    );
    expect(parsed).toEqual({
      html: '<p>hi</p>',
      text: 'hi',
      subject: 'Alert',
      sender: 'bank@x.com',
    });
  });

  it('accepts the snippet and from aliases', () => {
    const parsed = parseEmailEvidence(
      record({ raw_payload_json: JSON.stringify({ snippet: 'short', from: 'a@b.com' }) })
    );
    expect(parsed.text).toBe('short');
    expect(parsed.sender).toBe('a@b.com');
  });

  it('falls back to the snippet when the payload is absent or corrupt', () => {
    expect(parseEmailEvidence(record()).text).toBe('You spent Rs 31.52');
    expect(parseEmailEvidence(record({ raw_payload_json: '{not json' })).text).toBe(
      'You spent Rs 31.52'
    );
  });

  it('yields empty evidence when there is nothing stored at all', () => {
    expect(parseEmailEvidence(record({ body_snippet: null }))).toEqual({
      html: '',
      text: '',
      subject: '',
      sender: '',
    });
  });
});
