import { describe, it, expect } from 'vitest';
import { formatMoney } from './formatMoney';

describe('formatMoney', () => {
  it('renders an em dash when the amount is absent', () => {
    expect(formatMoney(null, 'USD')).toBe('—');
  });

  it('defaults to the rupee symbol when no currency is recorded', () => {
    expect(formatMoney(150000, null)).toBe('₹1,500.00');
  });

  it.each([
    ['USD', '$'],
    ['EUR', '€'],
    ['GBP', '£'],
  ])('uses the %s symbol', (currency, symbol) => {
    expect(formatMoney(2500, currency)).toBe(`${symbol}25.00`);
  });

  it('falls back to a prefixed code for currencies without a known symbol', () => {
    expect(formatMoney(2500, 'JPY')).toBe('JPY 25.00');
  });

  it('always shows two fraction digits', () => {
    expect(formatMoney(100, 'USD')).toBe('$1.00');
    expect(formatMoney(105, 'USD')).toBe('$1.05');
  });

  it('formats zero rather than treating it as missing', () => {
    expect(formatMoney(0, 'USD')).toBe('$0.00');
  });

  it('keeps the sign on negative (refund) amounts', () => {
    expect(formatMoney(-2500, 'USD')).toBe('$-25.00');
  });
});
