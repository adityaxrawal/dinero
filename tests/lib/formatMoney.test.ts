import { describe, it, expect } from 'vitest';
import { formatMoney, moneyTone, toMinor } from '@/lib/formatMoney';

describe('formatMoney', () => {
  it.each([null, undefined])('renders an em dash for %p', (v) => {
    expect(formatMoney(v, 'USD')).toBe('—');
  });

  it('defaults to rupees when no currency is recorded', () => {
    expect(formatMoney(150000, null)).toBe('₹1,500.00');
  });

  it('uses en-IN lakh grouping', () => {
    expect(formatMoney(12345678, 'INR')).toBe('₹1,23,456.78');
  });

  it("uses the row's currency, not a hard-coded rupee", () => {
    expect(formatMoney(2500, 'USD')).toBe('$25.00');
    expect(formatMoney(2500, 'EUR')).toBe('€25.00');
  });

  it('falls back to the code for an invalid currency', () => {
    expect(formatMoney(2500, 'XX1')).toBe('XX1 25.00');
  });

  it('always shows two fraction digits and formats zero', () => {
    expect(formatMoney(100, 'INR')).toBe('₹1.00');
    expect(formatMoney(0, 'INR')).toBe('₹0.00');
  });

  it('signs by direction so it is not shown by colour alone', () => {
    expect(formatMoney(2500, 'INR', 'debit')).toBe('−₹25.00');
    expect(formatMoney(-2500, 'INR', 'debit')).toBe('−₹25.00');
    expect(formatMoney(2500, 'INR', 'credit')).toBe('+₹25.00');
  });

  it('keeps the sign of a negative amount with no direction', () => {
    expect(formatMoney(-2500, 'INR')).toBe('−₹25.00');
  });
});

describe('moneyTone', () => {
  it('colours debits red and credits green', () => {
    expect(moneyTone('debit')).toBe('text-red-700');
    expect(moneyTone('credit')).toBe('text-emerald-700');
    expect(moneyTone(null)).toBe('');
  });
});

describe('toMinor', () => {
  it('rounds float major units to whole minor units', () => {
    expect(toMinor(0.1 + 0.2)).toBe(30);
    expect(toMinor(-1499)).toBe(-149900);
  });
});
