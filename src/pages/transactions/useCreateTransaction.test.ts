// Manual entry is the one write path into transactions that bypasses the
// extraction pipeline entirely, so its validation is worth pinning.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useCreateTransaction } from './useCreateTransaction';
import { API } from '@/lib/ipc';

const toast = vi.fn();
const invalidateQueries = vi.fn();

vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({ invalidateQueries }) }));
vi.mock('@/lib/ipc', () => ({ API: { transactions: { create: vi.fn() } } }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.clearAllMocks();
  asMock(API.transactions.create).mockResolvedValue(undefined);
});

function fill(result: { current: ReturnType<typeof useCreateTransaction> }) {
  act(() => {
    result.current.setMerchant('  Amazon  ');
    result.current.setAmount('245.50');
    result.current.setInstrumentId('inst-1');
    result.current.setDate('2026-08-09');
  });
}

describe('useCreateTransaction', () => {
  it('starts closed, with today prefilled', () => {
    const { result } = renderHook(() => useCreateTransaction());
    expect(result.current.isOpen).toBe(false);
    expect(result.current.date).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it('refuses to submit without a merchant, amount or instrument', async () => {
    const { result } = renderHook(() => useCreateTransaction());

    for (const partial of [
      { amount: '10', instrumentId: 'i1', merchant: '   ' },
      { amount: '', instrumentId: 'i1', merchant: 'A' },
      { amount: '10', instrumentId: '', merchant: 'A' },
    ]) {
      act(() => {
        result.current.setMerchant(partial.merchant);
        result.current.setAmount(partial.amount);
        result.current.setInstrumentId(partial.instrumentId);
      });
      await act(async () => {
        await result.current.submit();
      });
    }
    expect(API.transactions.create).not.toHaveBeenCalled();
  });

  it('refuses a zero or negative amount', async () => {
    const { result } = renderHook(() => useCreateTransaction());
    act(() => {
      result.current.setMerchant('A');
      result.current.setInstrumentId('i1');
      result.current.setAmount('0');
    });
    await act(async () => {
      await result.current.submit();
    });
    expect(API.transactions.create).not.toHaveBeenCalled();
  });

  it('creates in minor units, with the merchant trimmed and a day-start time', async () => {
    const { result } = renderHook(() => useCreateTransaction());
    fill(result);
    await act(async () => {
      await result.current.submit();
    });

    expect(API.transactions.create).toHaveBeenCalledWith({
      amountMinor: 24550,
      currency: 'INR',
      direction: 'debit',
      eventTime: '2026-08-09 00:00:00',
      merchantName: 'Amazon',
      instrumentId: 'inst-1',
    });
  });

  it('closes, clears the draft and refreshes the affected views on success', async () => {
    const { result } = renderHook(() => useCreateTransaction());
    act(() => result.current.setIsOpen(true));
    fill(result);
    await act(async () => {
      await result.current.submit();
    });

    expect(result.current.isOpen).toBe(false);
    expect(result.current.merchant).toBe('');
    expect(result.current.amount).toBe('');
    expect(result.current.direction).toBe('debit');
    expect(invalidateQueries).toHaveBeenCalledTimes(2);
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Transaction Created' }));
  });

  it('keeps the draft open and reports the failure when the create fails', async () => {
    asMock(API.transactions.create).mockRejectedValue(new Error('db locked'));
    const { result } = renderHook(() => useCreateTransaction());
    act(() => result.current.setIsOpen(true));
    fill(result);
    await act(async () => {
      await result.current.submit();
    });

    expect(result.current.isOpen).toBe(true);
    expect(result.current.merchant).toBe('  Amazon  ');
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ variant: 'destructive' }));
  });
});
