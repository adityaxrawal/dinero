import { describe, it, expect } from 'vitest';
import { formatMerchantName, getMerchantCategoryVisuals } from './merchantFormatter';

// Characterization tests written against the pre-refactor if-chains, so the
// table-driven rewrite is provably behaviour-preserving.

describe('formatMerchantName', () => {
  it('falls back for empty input', () => {
    expect(formatMerchantName('')).toBe('Unknown Merchant');
  });

  it.each([
    ['ZOMATOLIMITED', 'Zomato'],
    ['SWIGGY INSTAMART', 'Swiggy'],
    ['BLINKMERCEPVTLTD', 'Blinkit'],
    ['blinkit stores', 'Blinkit'],
    ['FLIPKART INTERNET', 'Flipkart'],
    ['AMAZON PAY INDIA', 'Amazon'],
    ['BHARTIAIRTELLTD', 'Airtel'],
    ['RELIANCE JIO INFOCOMM', 'Reliance Jio'],
    ['DREAMPLUG TECHNOLOGIES', 'CRED'],
    ['UBER INDIA', 'Uber'],
    ['OLA CABS', 'Ola'],
    ['NETFLIX ENTERTAINMENT', 'Netflix'],
    ['SPOTIFY INDIA', 'Spotify'],
    ['MAKEMYTRIP INDIA', 'MakeMyTrip'],
    ['BOOKMYSHOW', 'BookMyShow'],
    ['ZEPTO NOW', 'Zepto'],
  ])('maps %s to the known brand %s', (raw, expected) => {
    expect(formatMerchantName(raw)).toBe(expected);
  });

  it('matches brands before any suffix cleanup runs', () => {
    // "ola" is a substring of many words; pinned because the brand check wins.
    expect(formatMerchantName('SOLAR POWER LTD')).toBe('Ola');
  });

  it('title-cases an all-caps name with no corporate suffix', () => {
    expect(formatMerchantName('CORNER CAFE')).toBe('Corner Cafe');
  });

  it('double-spaces PVTLTD and skips title-casing (known limitation)', () => {
    // "PVTLTD" -> " Pvt Ltd", then the /LTD/gi rule fires again on the "Ltd" it
    // just produced -> " Pvt  Ltd". The now-mixed case also fails the
    // all-caps check, so "ACME" is never title-cased. Pinned as-is:
    // pre-existing behaviour, not introduced by the refactor.
    expect(formatMerchantName('ACMEPVTLTD')).toBe('ACME Pvt  Ltd');
  });

  it('expands a TECHNOLOGIES suffix', () => {
    expect(formatMerchantName('WIDGET TECHNOLOGIES')).toBe('WIDGET  Tech');
  });

  it('leaves already mixed-case names alone', () => {
    expect(formatMerchantName('Corner Cafe')).toBe('Corner Cafe');
  });

  it('trims surrounding whitespace', () => {
    expect(formatMerchantName('  Corner Cafe  ')).toBe('Corner Cafe');
  });
});

describe('getMerchantCategoryVisuals', () => {
  const styleOf = (category?: string, merchant?: string) => {
    const { bgClass, textClass } = getMerchantCategoryVisuals(category, merchant);
    return `${bgClass}|${textClass}`;
  };

  it.each([
    ['food', undefined, 'bg-amber-500/15 border-amber-500/20|text-amber-800'],
    [undefined, 'zomato', 'bg-amber-500/15 border-amber-500/20|text-amber-800'],
    ['shopping', undefined, 'bg-purple-500/15 border-purple-500/20|text-purple-800'],
    [undefined, 'amazon', 'bg-purple-500/15 border-purple-500/20|text-purple-800'],
    ['bills', undefined, 'bg-blue-500/15 border-blue-500/20|text-blue-800'],
    ['grocery', undefined, 'bg-emerald-500/15 border-emerald-500/20|text-emerald-800'],
    ['finance', undefined, 'bg-indigo-500/15 border-indigo-500/20|text-indigo-800'],
    ['travel', undefined, 'bg-sky-500/15 border-sky-500/20|text-sky-800'],
    ['entertainment', undefined, 'bg-rose-500/15 border-rose-500/20|text-rose-800'],
  ])('styles category=%s merchant=%s', (category, merchant, expected) => {
    expect(styleOf(category, merchant)).toBe(expected);
  });

  it('falls back to the neutral style for an unknown category', () => {
    expect(styleOf('quantum widgets', 'nobody')).toBe('bg-[#064E3B]/10 border-[#064E3B]/15|text-[#064E3B]');
  });

  it('falls back when nothing is supplied', () => {
    expect(styleOf()).toBe('bg-[#064E3B]/10 border-[#064E3B]/15|text-[#064E3B]');
  });

  it('prefers the earlier rule when a merchant matches two categories', () => {
    // "cred" hits Financial, but a food category is checked first.
    expect(styleOf('food', 'cred')).toBe('bg-amber-500/15 border-amber-500/20|text-amber-800');
  });

  it('always returns an icon element', () => {
    expect(getMerchantCategoryVisuals('food').icon).toBeTruthy();
  });
});
