import type { InstrumentRecord } from '@/lib/ipc';

export interface PortfolioMetrics {
  totalBankBalance: number;
  totalCreditSpent: number;
  totalCreditLimit: number;
  count: number;
}

/**
 * Portfolio totals for the accounts sidebar. Credit balances are stored
 * negative (money owed) and shown as a positive "spent" figure, so they are
 * summed by absolute value and kept out of the bank-balance total rather than
 * netting against it -- a card with ₹20k outstanding must not read as ₹20k of
 * available cash.
 */
export function portfolioMetrics(instruments: InstrumentRecord[]): PortfolioMetrics {
  let totalBankBalance = 0;
  let totalCreditSpent = 0;
  let totalCreditLimit = 0;

  for (const inst of instruments) {
    const bal = inst.current_balance ?? 0;
    if (inst.instrument_type === 'credit_card') {
      totalCreditSpent += Math.abs(bal);
      if (inst.credit_limit) {
        totalCreditLimit += inst.credit_limit;
      }
    } else if (inst.instrument_type === 'bank_account' || inst.instrument_type === 'wallet') {
      totalBankBalance += bal;
    }
  }

  return { totalBankBalance, totalCreditSpent, totalCreditLimit, count: instruments.length };
}
