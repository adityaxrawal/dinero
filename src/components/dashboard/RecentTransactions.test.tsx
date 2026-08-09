import {describe, it, expect, vi} from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import RecentTransactions from './RecentTransactions';
import { formatLastSynced } from './formatLastSynced';
import type { TransactionRecord } from '@/lib/ipc';

function tx(overrides: Partial<TransactionRecord> = {}): TransactionRecord {
  return {
    id: 'tx_1',
    date: '2026-07-20',
    merchant: 'Coffee Shop',
    amount: -450,
    direction: 'debit',
    category: 'Food',
    status: 'posted',
    source_mix: 'email_only',
    instrument_id: 'inst_1',
    ...overrides,
  };
}

function renderWithRouter(ui: React.ReactElement) {
  return render(<MemoryRouter>{ui}</MemoryRouter>);
}

describe('formatLastSynced', () => {
  it('near-real-time granularity, never claims instant/real-time delivery', () => {
    const now = new Date('2026-07-20T12:00:00Z');
    expect(formatLastSynced(new Date('2026-07-20T11:59:45Z'), now)).toBe('just now');
    expect(formatLastSynced(new Date('2026-07-20T11:59:00Z'), now)).toBe('1 min ago');
    expect(formatLastSynced(new Date('2026-07-20T11:55:00Z'), now)).toBe('5 mins ago');
    expect(formatLastSynced(new Date('2026-07-20T10:00:00Z'), now)).toBe('2h ago');
  });
});

describe('RecentTransactions', () => {
  it('test_ui_copy_never_claims_real_time: renders "near-real-time" copy, never the word "real-time" alone', () => {
    renderWithRouter(<RecentTransactions transactions={[tx()]} />);
    const label = screen.getByTestId('last-synced-label');
    expect(label.textContent).toMatch(/near-real-time/i);
    // "real-time" must never appear on its own -- only ever as part of
    // "near-real-time" -- so stripping every "near-real-time" occurrence
    // must leave no bare "real-time" behind.
    const withoutNearRealTime = label.textContent!.toLowerCase().replace(/near-real-time/g, '');
    expect(withoutNearRealTime).not.toMatch(/\breal-time\b/);
  });

  it('shows the empty state when there are no transactions', () => {
    renderWithRouter(<RecentTransactions transactions={[]} />);
    expect(screen.getByText('No transactions yet')).toBeTruthy();
  });

  it('test_new_row_gets_highlight_animation: a newly-arrived row is marked highlighted, an already-seen one is not', async () => {
    const { rerender } = renderWithRouter(
      <RecentTransactions transactions={[tx({ id: 'tx_1' })]} />
    );
    // First render establishes the baseline -- nothing is "new" yet.
    expect(screen.getByTestId('recent-tx-row-tx_1')).toHaveAttribute('data-highlighted', 'false');

    rerender(
      <MemoryRouter>
        <RecentTransactions transactions={[tx({ id: 'tx_2' }), tx({ id: 'tx_1' })]} />
      </MemoryRouter>
    );

    await waitFor(() => {
      expect(screen.getByTestId('recent-tx-row-tx_2')).toHaveAttribute('data-highlighted', 'true');
    });
    expect(screen.getByTestId('recent-tx-row-tx_1')).toHaveAttribute('data-highlighted', 'false');
  });

  it('the highlight clears itself after the animation duration', async () => {
    vi.useFakeTimers();
    const { rerender } = renderWithRouter(
      <RecentTransactions transactions={[tx({ id: 'tx_1' })]} />
    );
    rerender(
      <MemoryRouter>
        <RecentTransactions transactions={[tx({ id: 'tx_2' }), tx({ id: 'tx_1' })]} />
      </MemoryRouter>
    );
    expect(screen.getByTestId('recent-tx-row-tx_2')).toHaveAttribute('data-highlighted', 'true');

    vi.advanceTimersByTime(3000);
    await vi.waitFor(() => {
      expect(screen.getByTestId('recent-tx-row-tx_2')).toHaveAttribute('data-highlighted', 'false');
    });
    vi.useRealTimers();
  });
});
