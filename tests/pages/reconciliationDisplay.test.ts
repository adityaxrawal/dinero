import { describe, it, expect } from 'vitest';
import {
  readAlertPayload,
  stripAddressFromName,
  resolveBankName,
  formatRupees,
  getUnassignedDisplayInfo,
} from '@/pages/reconciliationDisplay';
import type { UnassignedTransactionRecord } from '@/lib/ipc';

const item = (over: Partial<UnassignedTransactionRecord> = {}): UnassignedTransactionRecord => ({
  id: 'u1',
  observation_id: 'obs1',
  reason: 'extraction_failed',
  status: 'pending',
  created_at: '2026-01-15T00:00:00Z',
  merchant_raw: null,
  amount_minor: 45050,
  currency: 'INR',
  direction: 'debit',
  event_time: '2026-01-15T10:00:00Z',
  source_message_id: 'msg1',
  body_snippet: 'Your card was debited',
  raw_payload_json: null,
  ...over,
});

describe('readAlertPayload', () => {
  it('falls back when the payload is absent', () => {
    expect(readAlertPayload(null, 'snippet')).toEqual({
      name: '',
      subject: '',
      html: '',
      text: 'snippet',
    });
  });

  it.each([undefined, 42, {}, []])('falls back for a non-string payload (%p)', (raw) => {
    expect(readAlertPayload(raw, 'snippet').text).toBe('snippet');
  });

  it('falls back when the payload is not valid JSON', () => {
    expect(readAlertPayload('{not json', 'snippet').text).toBe('snippet');
  });

  it('reads sender, subject, html and body', () => {
    const raw = JSON.stringify({
      sender: 'HDFC Bank',
      subject: 'Txn alert',
      html: '<p>hi</p>',
      body: 'full body',
    });
    expect(readAlertPayload(raw, 'snippet')).toEqual({
      name: 'HDFC Bank',
      subject: 'Txn alert',
      html: '<p>hi</p>',
      text: 'full body',
    });
  });

  it('accepts `from` as an alias for `sender`', () => {
    expect(readAlertPayload(JSON.stringify({ from: 'Axis Bank' }), '').name).toBe('Axis Bank');
  });

  it('prefers sender over from', () => {
    const raw = JSON.stringify({ sender: 'Preferred', from: 'Ignored' });
    expect(readAlertPayload(raw, '').name).toBe('Preferred');
  });

  it('keeps the snippet when the payload carries no body', () => {
    expect(readAlertPayload(JSON.stringify({ subject: 's' }), 'snippet').text).toBe('snippet');
  });
});

describe('stripAddressFromName', () => {
  it('leaves a bare display name alone', () => {
    expect(stripAddressFromName('IndusInd Bank')).toBe('IndusInd Bank');
  });

  it('drops an RFC822 address', () => {
    expect(stripAddressFromName('IndusInd Bank <alerts@indusind.com>')).toBe('IndusInd Bank');
  });

  it('strips surrounding quotes', () => {
    expect(stripAddressFromName('"HDFC Bank" <alerts@hdfc.com>')).toBe('HDFC Bank');
  });

  it('handles an address with no display name', () => {
    expect(stripAddressFromName('<alerts@hdfc.com>')).toBe('');
  });
});

describe('resolveBankName', () => {
  it('keeps a real sender name', () => {
    expect(resolveBankName('Kotak Bank', 'body mentioning HDFC')).toBe('Kotak Bank');
  });

  it.each(['', 'Bank Alert', 'Bank / Service Alert'])(
    'fingerprints the body when the name is %p',
    (name) => {
      expect(resolveBankName(name, 'Dear customer, your IndusInd card…')).toBe('IndusInd Bank');
    }
  );

  it.each([
    ['HDFC', 'HDFC Bank'],
    ['IndusInd', 'IndusInd Bank'],
    ['ICICI', 'ICICI Bank'],
    ['Axis', 'Axis Bank'],
    ['SBI', 'SBI Bank'],
    ['Kotak', 'Kotak Bank'],
  ])('recognises %s in the body', (needle, expected) => {
    expect(resolveBankName('', `alert from ${needle} today`)).toBe(expected);
  });

  it('matches the fingerprint case-insensitively', () => {
    expect(resolveBankName('', 'your hdfc card')).toBe('HDFC Bank');
  });

  it('returns the first matching fingerprint in declared order', () => {
    expect(resolveBankName('', 'HDFC and Axis both appear')).toBe('HDFC Bank');
  });

  it('falls back to a generic label when nothing matches', () => {
    expect(resolveBankName('', 'no recognisable bank here')).toBe('Bank Alert');
  });
});

describe('formatRupees', () => {
  it.each([null, undefined])('returns null for %p', (v) => {
    expect(formatRupees(v)).toBeNull();
  });

  it('uses en-IN lakh grouping', () => {
    expect(formatRupees(10000000)).toBe('₹1,00,000.00');
  });

  it('always shows two decimals', () => {
    expect(formatRupees(45050)).toBe('₹450.50');
    expect(formatRupees(100)).toBe('₹1.00');
  });

  it('formats zero rather than treating it as absent', () => {
    expect(formatRupees(0)).toBe('₹0.00');
  });
});

describe('getUnassignedDisplayInfo', () => {
  it('uses the payload sender as the display name', () => {
    const info = getUnassignedDisplayInfo(
      item({ raw_payload_json: JSON.stringify({ sender: 'Axis Bank <a@axis.com>' }) })
    );
    expect(info.name).toBe('Axis Bank');
    expect(info.avatarLetter).toBe('A');
  });

  it('falls back to merchant_raw when the payload has no sender', () => {
    expect(getUnassignedDisplayInfo(item({ merchant_raw: 'Kotak Bank' })).name).toBe('Kotak Bank');
  });

  it('fingerprints the body when neither names the bank', () => {
    const info = getUnassignedDisplayInfo(item({ body_snippet: 'Your ICICI card was used' }));
    expect(info.name).toBe('ICICI Bank');
  });

  it('prefers the subject as the snippet', () => {
    const info = getUnassignedDisplayInfo(
      item({ raw_payload_json: JSON.stringify({ sender: 'HDFC', subject: 'Txn of ₹450' }) })
    );
    expect(info.snippet).toBe('Txn of ₹450');
  });

  it('drops a bank-name echo at the start of the snippet', () => {
    const info = getUnassignedDisplayInfo(
      item({ merchant_raw: 'HDFC Bank', body_snippet: 'HDFC Bank debited your card' })
    );
    expect(info.name).toBe('HDFC Bank');
    expect(info.snippet).toBe('debited your card');
  });

  it('explains itself when there is nothing to show', () => {
    const info = getUnassignedDisplayInfo(item({ body_snippet: '', merchant_raw: '' }));
    expect(info.snippet).toBe('Transaction alert details missing');
  });

  it.each([
    ['extraction_failed', 'Missing Fields'],
    ['issuer_name_not_found', 'Unknown Card/Bank'],
    ['something_else', 'Action Needed'],
  ])('labels reason %s as %s', (reason, issueLabel) => {
    expect(getUnassignedDisplayInfo(item({ reason })).issueLabel).toBe(issueLabel);
  });

  it('formats the amount in rupees', () => {
    expect(getUnassignedDisplayInfo(item({ amount_minor: 45050 })).amountStr).toBe('₹450.50');
  });

  it('leaves the amount null when the extraction found none', () => {
    expect(getUnassignedDisplayInfo(item({ amount_minor: null })).amountStr).toBeNull();
  });

  it('renders a short date', () => {
    expect(getUnassignedDisplayInfo(item({ event_time: '2026-01-15T10:00:00Z' })).dateStr).toMatch(
      /Jan 1[45]/
    );
  });

  it('leaves the date blank when unknown', () => {
    expect(getUnassignedDisplayInfo(item({ event_time: null })).dateStr).toBe('');
  });

  it('uppercases the avatar letter', () => {
    expect(getUnassignedDisplayInfo(item({ merchant_raw: 'kotak' })).avatarLetter).toBe('K');
  });
});
