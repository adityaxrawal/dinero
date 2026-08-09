// Covers the cards extracted out of the old InstrumentDetail page.
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import EditableDetailsCard from './EditableDetailsCard';
import SavedPasswordsCard from './SavedPasswordsCard';
import RecentTransactionsCard from './RecentTransactionsCard';
import StatementHistoryCard from './StatementHistoryCard';

vi.mock('@/lib/formatCustomDate', () => ({ formatCustomDate: (d: string) => `on ${d}` }));

const setField = vi.fn();
const form = (over = {}) =>
  ({
    fields: { fullIdentifier: '4111', billingCycleDay: '15', bankIfsc: 'HDFC0001' },
    setField,
    isSaving: false,
    isDeleting: false,
    handleSave: vi.fn(),
    handleDelete: vi.fn(),
    recentTransactions: [],
    totalTxCount: 0,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
    ...over,
  }) as never;

const inst = (over = {}) =>
  ({
    id: 'i1',
    issuer_name: 'HDFC',
    instrument_type: 'credit_card',
    masked_identifier: '1234',
    current_balance: -2500.5,
    credit_limit: 150000,
    ...over,
  }) as never;

describe('EditableDetailsCard', () => {
  it('formats balance and limit, and shows an em dash for an unknown balance', () => {
    const { unmount } = render(<EditableDetailsCard form={form()} inst={inst()} />);
    expect(screen.getByText('₹-2500.50')).toBeInTheDocument();
    expect(screen.getByText('₹150000.00')).toBeInTheDocument();
    unmount();

    render(
      <EditableDetailsCard form={form()} inst={inst({ current_balance: null, credit_limit: null })} />
    );
    expect(screen.getByText('—')).toBeInTheDocument();
    expect(screen.queryByText('Credit Limit')).not.toBeInTheDocument();
  });

  it('offers the billing cycle day only for a credit card', () => {
    const { unmount } = render(<EditableDetailsCard form={form()} inst={inst()} />);
    expect(screen.getByLabelText('Billing Cycle Day')).toBeInTheDocument();
    expect(screen.queryByLabelText('IFSC Code')).not.toBeInTheDocument();
    unmount();

    render(<EditableDetailsCard form={form()} inst={inst({ instrument_type: 'bank_account' })} />);
    expect(screen.getByLabelText('IFSC Code')).toBeInTheDocument();
    expect(screen.queryByLabelText('Billing Cycle Day')).not.toBeInTheDocument();
  });

  it('edits the full identifier through setField', () => {
    render(<EditableDetailsCard form={form()} inst={inst()} />);
    fireEvent.change(screen.getByLabelText('Full Identifier'), { target: { value: '5555' } });
    expect(setField).toHaveBeenCalledWith('fullIdentifier', '5555');
  });
});

describe('SavedPasswordsCard', () => {
  it('pluralises the use count and forgets the right entry', () => {
    const onForget = vi.fn();
    render(
      <SavedPasswordsCard
        passwords={[
          { id: 'p1', success_count: 1 },
          { id: 'p2', success_count: 4 },
        ] as never}
        onForget={onForget}
        isForgetting={false}
      />
    );
    expect(screen.getByText('Used 1 time')).toBeInTheDocument();
    expect(screen.getByText('Used 4 times')).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole('button', { name: 'Forget Password' })[1]);
    expect(onForget).toHaveBeenCalledWith('p2');
  });
});

describe('RecentTransactionsCard', () => {
  const txns = [
    { id: 't1', merchant: 'Swiggy', amount: -245.5, direction: 'debit' },
    { id: 't2', merchant: 'Salary', amount: 50000, direction: 'credit' },
  ];

  it('says so plainly when there is nothing yet', () => {
    render(<RecentTransactionsCard form={form()} onViewAll={vi.fn()} />);
    expect(screen.getByText('No transactions for this instrument yet.')).toBeInTheDocument();
  });

  it('signs debits and credits differently', () => {
    render(
      <RecentTransactionsCard
        form={form({ recentTransactions: txns, totalTxCount: 2 })}
        onViewAll={vi.fn()}
      />
    );
    expect(screen.getByText('DEBIT')).toBeInTheDocument();
    expect(screen.getByText('CREDIT')).toBeInTheDocument();
    expect(screen.getByText('−₹245.50')).toBeInTheDocument();
    expect(screen.getByText('+₹50,000.00')).toBeInTheDocument();
    expect(screen.getByText('Showing 2 of 2 transactions')).toBeInTheDocument();
  });

  it('pages only while more remain', () => {
    const fetchNextPage = vi.fn();
    render(
      <RecentTransactionsCard
        form={form({ recentTransactions: txns, totalTxCount: 9, hasNextPage: true, fetchNextPage })}
        onViewAll={vi.fn()}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: /Load More Transactions \(2 of 9\)/ }));
    expect(fetchNextPage).toHaveBeenCalled();
  });
});

describe('StatementHistoryCard', () => {
  it('says so plainly when empty, and lists status otherwise', () => {
    const { unmount } = render(<StatementHistoryCard statements={[] as never} />);
    expect(screen.getByText('No statements for this instrument yet.')).toBeInTheDocument();
    unmount();

    render(
      <StatementHistoryCard
        statements={
          [{ id: 's1', file_name: 'jul.pdf', date: '2026-07-01', status: 'PROCESSED' }] as never
        }
      />
    );
    expect(screen.getByText('jul.pdf')).toBeInTheDocument();
    expect(screen.getByText('PROCESSED')).toBeInTheDocument();
    expect(screen.getByText('on 2026-07-01')).toBeInTheDocument();
  });
});
