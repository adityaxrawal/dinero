import { describe, it, expect } from 'vitest';
import { portfolioMetrics } from '@/pages/instruments/portfolioMetrics';
import type { InstrumentRecord } from '@/lib/ipc';

const inst = (over: Partial<InstrumentRecord>): InstrumentRecord =>
  ({
    id: 'i1',
    instrument_type: 'bank_account',
    issuer_name: 'HDFC',
    masked_identifier: '1234',
    status: 'active',
    ...over,
  }) as InstrumentRecord;

describe('portfolioMetrics', () => {
  it('returns zeroes for an empty portfolio', () => {
    expect(portfolioMetrics([])).toEqual({
      totalBankBalance: 0,
      totalCreditSpent: 0,
      totalCreditLimit: 0,
      count: 0,
    });
  });

  it('sums bank accounts and wallets into the balance', () => {
    const got = portfolioMetrics([
      inst({ instrument_type: 'bank_account', current_balance: 5000 }),
      inst({ instrument_type: 'wallet', current_balance: 250 }),
    ]);
    expect(got.totalBankBalance).toBe(5250);
  });

  it('reports credit owed as a positive spend, not a negative balance', () => {
    // Credit balances are stored negative; netting them against cash would
    // report ₹20k of debt as ₹20k of available money.
    const got = portfolioMetrics([
      inst({ instrument_type: 'bank_account', current_balance: 5000 }),
      inst({ instrument_type: 'credit_card', current_balance: -20000, credit_limit: 150000 }),
    ]);
    expect(got.totalBankBalance).toBe(5000);
    expect(got.totalCreditSpent).toBe(20000);
    expect(got.totalCreditLimit).toBe(150000);
  });

  it('treats a missing balance as zero', () => {
    const got = portfolioMetrics([inst({ instrument_type: 'bank_account' })]);
    expect(got.totalBankBalance).toBe(0);
  });

  it('skips a credit card with no stated limit', () => {
    const got = portfolioMetrics([
      inst({ instrument_type: 'credit_card', current_balance: -100 }),
      inst({ instrument_type: 'credit_card', current_balance: -50, credit_limit: 90000 }),
    ]);
    expect(got.totalCreditSpent).toBe(150);
    expect(got.totalCreditLimit).toBe(90000);
  });

  it('counts every instrument, including types it does not total', () => {
    const got = portfolioMetrics([
      inst({ instrument_type: 'upi_vpa' }),
      inst({ instrument_type: 'bank_account', current_balance: 10 }),
    ]);
    expect(got.count).toBe(2);
    expect(got.totalBankBalance).toBe(10);
  });
});
