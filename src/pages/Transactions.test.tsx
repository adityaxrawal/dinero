import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import Transactions from './Transactions';
import { API } from '@/lib/ipc';

const toast = vi.fn();
const invalidateQueries = vi.fn();
let transactions: Array<Record<string, unknown>> = [];
let isLoading = false;

vi.mock('react-router-dom', () => ({
  useSearchParams: () => [new URLSearchParams(), vi.fn()],
}));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({ invalidateQueries }) }));
vi.mock('@/lib/ipc', () => ({ API: { transactions: { create: vi.fn(), export: vi.fn() } } }));
vi.mock('@/hooks/queries/useTransactionsInfiniteList', () => ({
  useTransactionsInfiniteList: () => ({
    data: { pages: [{ records: transactions }] },
    isLoading,
    fetchNextPage: vi.fn(),
    hasNextPage: false,
    isFetchingNextPage: false,
  }),
}));
vi.mock('@/hooks/queries/useTransactionSearch', () => ({
  useTransactionSearch: () => ({ data: undefined, isLoading: false }),
}));
vi.mock('@/hooks/queries/useInstrumentsList', () => ({
  useInstrumentsList: () => ({
    data: [{ id: 'inst1', issuer_name: 'HDFC Bank', masked_identifier: '8841', instrument_type: 'credit_card' }],
  }),
}));
vi.mock('@/hooks/queries/useCategoriesList', () => ({ useCategoriesList: () => ({ data: [] }) }));
vi.mock('@/components/transactions/TransactionInspector', () => ({
  default: ({ transactionId }: { transactionId: string | null }) => (
    <div data-testid="inspector">{transactionId ?? 'none'}</div>
  ),
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const tx = (over: Record<string, unknown> = {}) => ({
  id: 't1',
  merchant: 'Swiggy',
  amount: 450.5,
  amount_minor: 45050,
  direction: 'debit',
  currency: 'INR',
  date: new Date().toISOString(),
  instrument_id: 'inst1',
  category_name: null,
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
  isLoading = false;
  transactions = [tx()];
  asMock(API.transactions.create).mockResolvedValue(undefined);
});

describe('Transactions list', () => {
  it('groups transactions under a relative date heading', () => {
    render(<Transactions />);
    expect(screen.getByText('Today')).toBeInTheDocument();
    expect(screen.getByText('Swiggy')).toBeInTheDocument();
  });

  it('starts a new group when the date label changes', () => {
    const yesterday = new Date(Date.now() - 864e5).toISOString();
    transactions = [tx(), tx({ id: 't2', date: yesterday, merchant: 'Zomato' })];
    render(<Transactions />);
    expect(screen.getByText('Today')).toBeInTheDocument();
    expect(screen.getByText('Yesterday')).toBeInTheDocument();
  });

  it('keeps same-day transactions in one group', () => {
    transactions = [tx(), tx({ id: 't2', merchant: 'Zomato' })];
    render(<Transactions />);
    expect(screen.getAllByText('Today')).toHaveLength(1);
  });

  it('selects a transaction on click', () => {
    render(<Transactions />);
    fireEvent.click(screen.getByText('Swiggy'));
    expect(screen.getByTestId('inspector').textContent).toBe('t1');
  });
});

describe('keyboard navigation', () => {
  beforeEach(() => {
    transactions = [
      tx({ id: 't1', merchant: 'First' }),
      tx({ id: 't2', merchant: 'Second' }),
      tx({ id: 't3', merchant: 'Third' }),
    ];
  });

  /** The inspector only mounts once a row is selected. */
  const selected = () => screen.queryByTestId('inspector')?.textContent ?? 'none';

  it.each(['ArrowDown', 'j'])('moves down with %s', (key) => {
    render(<Transactions />);
    fireEvent.keyDown(window, { key });
    expect(selected()).toBe('t1');
  });

  it.each(['ArrowUp', 'k'])('moves up with %s', (key) => {
    render(<Transactions />);
    fireEvent.keyDown(window, { key: 'ArrowDown' });
    fireEvent.keyDown(window, { key: 'ArrowDown' });
    fireEvent.keyDown(window, { key });
    expect(selected()).toBe('t1');
  });

  it('stops at the last item', () => {
    render(<Transactions />);
    for (let i = 0; i < 10; i++) fireEvent.keyDown(window, { key: 'ArrowDown' });
    expect(selected()).toBe('t3');
  });

  it('stops at the first item', () => {
    render(<Transactions />);
    for (let i = 0; i < 5; i++) fireEvent.keyDown(window, { key: 'ArrowUp' });
    expect(selected()).toBe('t1');
  });

  it('ignores keys typed into an input', () => {
    render(<Transactions />);
    const search = screen.getByPlaceholderText(/Search/i);
    fireEvent.keyDown(search, { key: 'j' });
    expect(selected()).toBe('none');
  });

  it('does nothing when the list is empty', () => {
    transactions = [];
    render(<Transactions />);
    fireEvent.keyDown(window, { key: 'ArrowDown' });
    expect(selected()).toBe('none');
  });

  it('stops listening after unmount', () => {
    const { unmount } = render(<Transactions />);
    unmount();
    expect(() => fireEvent.keyDown(window, { key: 'ArrowDown' })).not.toThrow();
  });
});

describe('creating a manual transaction', () => {
  const openDialog = () => {
    render(<Transactions />);
    fireEvent.click(screen.getByRole('button', { name: /Add|New|Manual/i }));
  };

  const fill = ({ amount = '450.50', merchant = 'Swiggy' } = {}) => {
    const amountInput = screen.getByLabelText(/Amount/i);
    fireEvent.change(amountInput, { target: { value: amount } });
    fireEvent.change(screen.getByLabelText(/Merchant/i), { target: { value: merchant } });
  };

  it('refuses to submit without an instrument', async () => {
    openDialog();
    fill();
    fireEvent.click(screen.getByRole('button', { name: /^Create|Add Transaction$/i }));
    await waitFor(() => expect(API.transactions.create).not.toHaveBeenCalled());
  });

  it.each([
    ['a non-numeric amount', { amount: 'abc' }],
    ['a zero amount', { amount: '0' }],
    ['a negative amount', { amount: '-5' }],
    ['a blank merchant', { merchant: '   ' }],
  ])('refuses to submit with %s', async (_label, over) => {
    openDialog();
    fill(over);
    fireEvent.click(screen.getByRole('button', { name: /^Create|Add Transaction$/i }));
    await waitFor(() => expect(API.transactions.create).not.toHaveBeenCalled());
  });
});
