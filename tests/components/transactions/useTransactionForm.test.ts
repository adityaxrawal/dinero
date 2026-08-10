import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useTransactionForm } from '@/components/transactions/useTransactionForm';

const toast = vi.fn();
const updateMutate = vi.fn();
const addTagMutate = vi.fn();
const removeTagMutate = vi.fn();
const softDeleteMutate = vi.fn();
const confirmDeleteTransaction = vi.fn();

let detail: unknown;
let tags: string[] = [];
let instruments: Array<{ id: string; issuer_name: string }> = [];
let categories: Array<{ id: string; name: string }> = [];

vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/hooks/queries/useTransactionDetail', () => ({
  useTransactionDetail: () => ({ data: detail, isLoading: false }),
}));
vi.mock('@/hooks/queries/useTransactionTags', () => ({ useTransactionTags: () => ({ data: tags }) }));
vi.mock('@/hooks/queries/useTagsList', () => ({ useTagsList: () => ({ data: [] }) }));
vi.mock('@/hooks/queries/useInstrumentsList', () => ({
  useInstrumentsList: () => ({ data: instruments }),
}));
vi.mock('@/hooks/queries/useCategoriesList', () => ({ useCategoriesList: () => ({ data: categories }) }));
vi.mock('@/hooks/mutations/useUpdateTransactionFields', () => ({
  useUpdateTransactionFields: () => ({ mutate: updateMutate, isPending: false }),
}));
vi.mock('@/hooks/mutations/useAddTransactionTag', () => ({
  useAddTransactionTag: () => ({ mutate: addTagMutate }),
}));
vi.mock('@/hooks/mutations/useRemoveTransactionTag', () => ({
  useRemoveTransactionTag: () => ({ mutate: removeTagMutate }),
}));
vi.mock('@/hooks/mutations/useSoftDeleteTransaction', () => ({
  useSoftDeleteTransaction: () => ({ mutate: softDeleteMutate, isPending: false }),
}));
vi.mock('@/lib/confirmDialog', () => ({
  confirmDeleteTransaction: () => confirmDeleteTransaction(),
}));

const transaction = (over = {}) => ({
  id: 'tx1',
  merchant_display_name: 'Swiggy',
  category_id: 'cat1',
  notes: 'lunch',
  amount: 450.5,
  amount_minor: 45050,
  direction: 'debit',
  best_event_time: '2026-01-01T10:00:00Z',
  instrument_id: 'inst1',
  emi_group_id: null,
  original_amount_minor: null,
  original_currency: null,
  currency: 'INR',
  ...over,
});

const setup = (id: string | undefined = 'tx1', onClose?: () => void) =>
  renderHook(() => useTransactionForm(id, onClose));

/** A panel opened with no transaction selected — every action must no-op. */
const setupWithoutId = () => renderHook(() => useTransactionForm(undefined));

beforeEach(() => {
  vi.clearAllMocks();
  detail = { transaction: transaction() };
  tags = [];
  instruments = [{ id: 'inst1', issuer_name: 'HDFC' }];
  categories = [{ id: 'cat1', name: 'Food' }];
});

describe('form hydration', () => {
  it('seeds every field from the loaded transaction', () => {
    const { result } = setup();
    expect(result.current).toMatchObject({
      merchant: 'Swiggy',
      categoryId: 'cat1',
      notes: 'lunch',
      amountStr: '450.5',
      direction: 'debit',
      eventTime: '2026-01-01T10:00:00Z',
      instrumentId: 'inst1',
    });
  });

  it('leaves fields blank until the transaction loads', () => {
    detail = undefined;
    const { result } = setup();
    expect(result.current.merchant).toBe('');
    expect(result.current.amountStr).toBe('');
  });

  it('substitutes empty strings for null columns', () => {
    detail = {
      transaction: transaction({
        merchant_display_name: null,
        category_id: null,
        notes: null,
        best_event_time: null,
        instrument_id: null,
      }),
    };
    const { result } = setup();
    expect(result.current).toMatchObject({ merchant: '', categoryId: '', notes: '', instrumentId: '' });
  });

  it('derives the amount from minor units when the decimal amount is absent', () => {
    detail = { transaction: transaction({ amount: null, amount_minor: 45050 }) };
    const { result } = setup();
    expect(result.current.amountStr).toBe('450.5');
  });

  it('shows a credit amount as a positive magnitude', () => {
    detail = { transaction: transaction({ amount: -450.5, direction: 'credit' }) };
    const { result } = setup();
    expect(result.current.amountStr).toBe('450.5');
    expect(result.current.direction).toBe('credit');
  });

  it('treats any non-credit direction as a debit', () => {
    detail = { transaction: transaction({ direction: 'unknown' }) };
    const { result } = setup();
    expect(result.current.direction).toBe('debit');
    expect(result.current.isDebit).toBe(true);
  });

  it('re-hydrates when a different transaction loads', () => {
    const { result, rerender } = setup();
    act(() => result.current.setMerchant('edited'));
    detail = { transaction: transaction({ merchant_display_name: 'Zomato' }) };
    rerender();
    expect(result.current.merchant).toBe('Zomato');
  });
});

describe('derived values', () => {
  it('resolves the selected instrument and category', () => {
    const { result } = setup();
    expect(result.current.instrument).toMatchObject({ id: 'inst1' });
    expect(result.current.category).toMatchObject({ id: 'cat1' });
  });

  it('leaves the instrument undefined when none is selected', () => {
    detail = { transaction: transaction({ instrument_id: null }) };
    const { result } = setup();
    expect(result.current.instrument).toBeUndefined();
  });

  it('flags a transaction that belongs to an EMI group', () => {
    detail = { transaction: transaction({ emi_group_id: 'emi1' }) };
    const { result } = setup();
    expect(result.current.hasEmi).toBe(true);
  });

  it('flags a foreign-currency transaction', () => {
    detail = {
      transaction: transaction({ original_amount_minor: 2500, original_currency: 'USD', currency: 'INR' }),
    };
    const { result } = setup();
    expect(result.current.isForeignCurrency).toBe(true);
  });

  it('does not flag a same-currency original amount as foreign', () => {
    detail = {
      transaction: transaction({ original_amount_minor: 45050, original_currency: 'INR', currency: 'INR' }),
    };
    const { result } = setup();
    expect(result.current.isForeignCurrency).toBe(false);
  });

  it('falls back to the stored amount when the field is not a number', () => {
    const { result } = setup();
    act(() => result.current.setAmountStr('abc'));
    expect(result.current.amount).toBe(450.5);
  });
});

describe('dirty tracking', () => {
  it('starts clean', () => {
    expect(setup().result.current.isDirty).toBe(false);
  });

  it.each([
    ['setMerchant', 'Zomato'],
    ['setCategoryId', 'cat2'],
    ['setNotes', 'dinner'],
    ['setAmountStr', '500'],
    ['setEventTime', '2026-02-01T00:00:00Z'],
    ['setInstrumentId', 'inst2'],
  ] as const)('goes dirty after %s', (setter, value) => {
    const { result } = setup();
    act(() => (result.current[setter] as (v: string) => void)(value));
    expect(result.current.isDirty).toBe(true);
  });

  it('goes dirty when the direction flips', () => {
    const { result } = setup();
    act(() => result.current.setDirection('credit'));
    expect(result.current.isDirty).toBe(true);
  });

  it('goes clean again after resetForm', () => {
    const { result } = setup();
    act(() => result.current.setMerchant('Zomato'));
    act(() => result.current.resetForm());
    expect(result.current.merchant).toBe('Swiggy');
    expect(result.current.isDirty).toBe(false);
  });
});

describe('handleSave', () => {
  it('does nothing without a transaction id', () => {
    const { result } = setupWithoutId();
    act(() => result.current.handleSave());
    expect(updateMutate).not.toHaveBeenCalled();
  });

  it('converts the amount to minor units', () => {
    const { result } = setup();
    act(() => result.current.setAmountStr('123.45'));
    act(() => result.current.handleSave());
    expect(updateMutate).toHaveBeenCalledWith(
      expect.objectContaining({ transactionId: 'tx1', amountMinor: 12345 }),
      expect.anything()
    );
  });

  it('rounds sub-paisa input rather than truncating', () => {
    const { result } = setup();
    act(() => result.current.setAmountStr('10.005'));
    act(() => result.current.handleSave());
    expect(updateMutate.mock.calls[0][0].amountMinor).toBe(1001);
  });

  it('omits the amount entirely when the field is unparseable', () => {
    const { result } = setup();
    act(() => result.current.setAmountStr('not a number'));
    act(() => result.current.handleSave());
    expect(updateMutate.mock.calls[0][0].amountMinor).toBeUndefined();
  });

  it('sends undefined rather than an empty string for blank optionals', () => {
    const { result } = setup();
    act(() => result.current.setEventTime(''));
    act(() => result.current.setInstrumentId(''));
    act(() => result.current.handleSave());
    expect(updateMutate.mock.calls[0][0]).toMatchObject({
      eventTime: undefined,
      instrumentId: undefined,
    });
  });

  it('shows a transient saved confirmation', async () => {
    vi.useFakeTimers();
    const { result } = setup();
    act(() => result.current.handleSave());
    act(() => updateMutate.mock.calls[0][1].onSuccess());
    expect(result.current.showSavedConfirm).toBe(true);
    act(() => vi.advanceTimersByTime(3000));
    expect(result.current.showSavedConfirm).toBe(false);
    vi.useRealTimers();
  });

  it('reports a save failure as a destructive toast', () => {
    const { result } = setup();
    act(() => result.current.handleSave());
    act(() => updateMutate.mock.calls[0][1].onError(new Error('db locked')));
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ variant: 'destructive', description: 'db locked' })
    );
  });
});

describe('tags', () => {
  it('adds a trimmed tag and clears the input', () => {
    const { result } = setup();
    act(() => result.current.setNewTag('  food  '));
    act(() => result.current.handleAddTag());
    expect(addTagMutate).toHaveBeenCalledWith(
      { transactionId: 'tx1', tagName: 'food' },
      expect.anything()
    );
    expect(result.current.newTag).toBe('');
  });

  it.each([['', 'blank'], ['   ', 'whitespace-only']])('ignores a %s tag (%s)', (value) => {
    const { result } = setup();
    act(() => result.current.setNewTag(value));
    act(() => result.current.handleAddTag());
    expect(addTagMutate).not.toHaveBeenCalled();
  });

  it('refuses to add a duplicate tag', () => {
    tags = ['food'];
    const { result } = setup();
    act(() => result.current.setNewTag('food'));
    act(() => result.current.handleAddTag());
    expect(addTagMutate).not.toHaveBeenCalled();
  });

  it('removes a tag', () => {
    const { result } = setup();
    act(() => result.current.handleRemoveTag('food'));
    expect(removeTagMutate).toHaveBeenCalledWith(
      { transactionId: 'tx1', tagName: 'food' },
      expect.anything()
    );
  });

  it('does not remove without a transaction id', () => {
    const { result } = setupWithoutId();
    act(() => result.current.handleRemoveTag('food'));
    expect(removeTagMutate).not.toHaveBeenCalled();
  });

  it('reports a tag failure as a destructive toast', () => {
    const { result } = setup();
    act(() => result.current.handleRemoveTag('food'));
    act(() => removeTagMutate.mock.calls[0][1].onError(new Error('nope')));
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ variant: 'destructive' }));
  });
});

describe('handleDelete', () => {
  it('asks for confirmation before deleting', async () => {
    confirmDeleteTransaction.mockResolvedValue(false);
    const { result } = setup();
    await act(async () => result.current.handleDelete());
    expect(confirmDeleteTransaction).toHaveBeenCalled();
    expect(softDeleteMutate).not.toHaveBeenCalled();
  });

  it('deletes and closes the panel on success', async () => {
    confirmDeleteTransaction.mockResolvedValue(true);
    const onClose = vi.fn();
    const { result } = setup('tx1', onClose);
    await act(async () => result.current.handleDelete());
    await waitFor(() => expect(softDeleteMutate).toHaveBeenCalledWith('tx1', expect.anything()));
    act(() => softDeleteMutate.mock.calls[0][1].onSuccess());
    expect(toast).toHaveBeenCalledWith({ title: 'Transaction deleted' });
    expect(onClose).toHaveBeenCalled();
  });

  it('survives having no onClose callback', async () => {
    confirmDeleteTransaction.mockResolvedValue(true);
    const { result } = setup();
    await act(async () => result.current.handleDelete());
    await waitFor(() => expect(softDeleteMutate).toHaveBeenCalled());
    expect(() => act(() => softDeleteMutate.mock.calls[0][1].onSuccess())).not.toThrow();
  });

  it('explains that only manual transactions are deletable', async () => {
    confirmDeleteTransaction.mockResolvedValue(true);
    const { result } = setup();
    await act(async () => result.current.handleDelete());
    await waitFor(() => expect(softDeleteMutate).toHaveBeenCalled());
    act(() => softDeleteMutate.mock.calls[0][1].onError({}));
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({
        variant: 'destructive',
        description: 'Only manually-entered transactions can be deleted.',
      })
    );
  });

  it('does nothing without a transaction id', async () => {
    const { result } = setupWithoutId();
    await act(async () => result.current.handleDelete());
    expect(confirmDeleteTransaction).not.toHaveBeenCalled();
  });
});
