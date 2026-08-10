/**
 * Aggregates instrument balances into portfolio totals.
 *
 * Credit and debit instruments are summed differently: a credit card balance is
 * money owed, not money held, so treating them alike would misstate net worth.
 */
import type { InstrumentRecord } from '@/lib/ipc';

export interface PortfolioMetrics {
  totalBankBalance: number;
  totalCreditSpent: number;
  totalCreditLimit: number;
  count: number;
}

/**
 * Aggregates instrument balances into portfolio totals.
 *
 * Credit and debit instruments are summed differently: a credit card balance is
 * money owed, not money held, so treating them alike would misstate net worth.
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
