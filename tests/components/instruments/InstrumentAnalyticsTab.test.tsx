import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import InstrumentAnalyticsTab from '@/components/instruments/InstrumentAnalyticsTab';
import type { TransactionRecord } from '@/lib/ipc';

describe('InstrumentAnalyticsTab', () => {
  it('correctly categorizes debit transactions as outflow and credit transactions as inflow', () => {
    const transactions: TransactionRecord[] = [
      {
        id: 'tx1',
        date: '2026-07-04',
        merchant: 'Airtel',
        amount: 588.82, // positive magnitude in DB
        direction: 'debit', // expense / outflow
        category: 'Utilities',
        status: 'posted',
        source_mix: null,
        instrument_id: 'inst1',
      },
      {
        id: 'tx2',
        date: '2026-05-25',
        merchant: 'Scribd',
        amount: 313.95, // positive magnitude in DB
        direction: 'debit', // expense / outflow
        category: 'Subscriptions',
        status: 'posted',
        source_mix: null,
        instrument_id: 'inst1',
      },
      {
        id: 'tx3',
        date: '2026-06-01',
        merchant: 'Salary',
        amount: 1000.00, // positive magnitude in DB
        direction: 'credit', // income / inflow
        category: 'Income',
        status: 'posted',
        source_mix: null,
        instrument_id: 'inst1',
      },
    ];

    render(<InstrumentAnalyticsTab transactions={transactions} />);

    // Total Outflow should be 588.82 + 313.95 = 902.77
    // Total Inflow should be 1000.00
    expect(screen.getByText('Total Outflow')).toBeInTheDocument();
    expect(screen.getByText(/902\.77/)).toBeInTheDocument();

    expect(screen.getByText('Total Inflow')).toBeInTheDocument();
    expect(screen.getByText(/1,000\.00/)).toBeInTheDocument();
  });
});
