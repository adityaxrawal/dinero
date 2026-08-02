import { describe, it, expect } from 'vitest';
import { extractQuickCandidates, parseSenderInfo } from './gmailParsing';

// Characterization tests: these pin down the behaviour of the two parsers that
// read money, dates and sender identity out of bank alert emails. They were
// written against the pre-refactor implementation so the decomposition that
// follows is provably behaviour-preserving.

describe('extractQuickCandidates', () => {
  it('returns nothing for empty input', () => {
    expect(extractQuickCandidates('')).toEqual([]);
  });

  it('pulls amounts written as INR, Rs. and ₹', () => {
    const got = extractQuickCandidates('INR 450.00 and Rs. 1,200 and ₹500.00');
    expect(got.filter((c) => c.type === 'amount')).toEqual([
      { type: 'amount', label: 'Amount: ₹450', value: '450.00' },
      { type: 'amount', label: 'Amount: ₹1,200', value: '1200.00' },
      { type: 'amount', label: 'Amount: ₹500', value: '500.00' },
    ]);
  });

  it('strips thousands separators when de-duplicating amounts', () => {
    // "1,200" and "1200" normalise to the same value and must appear once.
    const got = extractQuickCandidates('INR 1,200 paid, INR 1200 refunded');
    expect(got.filter((c) => c.type === 'amount')).toHaveLength(1);
  });

  it('drops zero and unparseable amounts', () => {
    expect(extractQuickCandidates('INR 0').filter((c) => c.type === 'amount')).toEqual([]);
  });

  it('normalises dates to ISO', () => {
    const got = extractQuickCandidates('on 2026-07-26 and 26-Jul-2026');
    const dates = got.filter((c) => c.type === 'date').map((c) => c.value);
    expect(dates).toContain('2026-07-26');
  });

  it('extracts reference ids but rejects generic words', () => {
    const got = extractQuickCandidates('Ref: ABC12345 txn: account');
    const refs = got.filter((c) => c.type === 'ref').map((c) => c.value);
    expect(refs).toContain('ABC12345');
    // "account" matches the id shape but is filtered as a generic word.
    expect(refs).not.toContain('account');
  });

  it('does not match a reference id separated from its keyword', () => {
    // "Ref No: ABC12345" — the regex expects the id immediately after the
    // keyword, and "No" is too short to be one, so nothing is captured.
    expect(extractQuickCandidates('Ref No: ABC12345').filter((c) => c.type === 'ref')).toEqual([]);
  });

  it('extracts a merchant name', () => {
    const got = extractQuickCandidates('spent at SWIGGY.');
    expect(got.filter((c) => c.type === 'merchant').map((c) => c.value)).toContain('SWIGGY');
  });

  it('captures trailing words into the merchant name (known limitation)', () => {
    // The merchant class is [A-Z0-9\s&] but the regex carries the `i` flag, so
    // it matches lowercase too and \s lets it run past the merchant name.
    // Pinned as-is: pre-existing behaviour, not introduced by the refactor.
    const got = extractQuickCandidates('spent at SWIGGY and paid to YOUR');
    expect(got.filter((c) => c.type === 'merchant').map((c) => c.value)).toEqual(['SWIGGY and paid to']);
  });

  it('caps the candidate list at six', () => {
    const text = 'INR 1 INR 2 INR 3 INR 4 INR 5 INR 6 INR 7 INR 8 INR 9';
    expect(extractQuickCandidates(text)).toHaveLength(6);
  });

  it('orders candidates amounts, then dates, then refs, then merchants', () => {
    const got = extractQuickCandidates('INR 99 on 2026-07-26 ref: ABC12345 spent at SWIGGY');
    expect(got.map((c) => c.type)).toEqual(['amount', 'date', 'ref', 'merchant']);
  });
});

describe('parseSenderInfo', () => {
  it('splits an RFC 822 sender into name and address', () => {
    expect(parseSenderInfo('IndusInd Bank <alerts@indusind.com>')).toEqual({
      displayName: 'IndusInd Bank',
      displayEmail: 'alerts@indusind.com',
    });
  });

  it('derives a bank name from a bare address', () => {
    expect(parseSenderInfo('alerts@hdfcbank.net')).toEqual({
      displayName: 'Hdfcbank Bank',
      displayEmail: 'alerts@hdfcbank.net',
    });
  });

  it('falls back to a known bank name found in the body', () => {
    const got = parseSenderInfo(null, null, 'Your HDFC Bank card was used for INR 500');
    expect(got.displayName).toBe('HDFC Bank');
  });

  it('recovers an address from the body when headers lack one', () => {
    const got = parseSenderInfo(null, null, 'Contact us at alerts@icicibank.com for help');
    expect(got.displayEmail).toBe('alerts@icicibank.com');
  });

  it('ignores placeholder addresses when scanning the body', () => {
    const got = parseSenderInfo(null, null, 'see example.com or foo@example.com');
    expect(got.displayEmail).not.toBe('foo@example.com');
  });

  it('defaults to Bank Alert when nothing is available', () => {
    expect(parseSenderInfo(null, null, null).displayName).toBe('Bank Alert');
  });
});
