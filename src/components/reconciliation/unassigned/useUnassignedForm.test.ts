// Saving an unassigned record is the manual escape hatch when extraction
// failed, so its conversion to wire units and its guards are pinned here.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useUnassignedForm } from './useUnassignedForm';
import type { UnassignedTransactionRecord } from '@/lib/ipc';

const mutate = vi.fn();
const toast = vi.fn();
const invalidateQueries = vi.fn();
const dismissUnassigned = vi.fn();
const onClose = vi.fn();

vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({ invalidateQueries }) }));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/lib/errorMapping', () => ({ getErrorToast: () => ({ title: 'failed' }) }));
vi.mock('@/hooks/mutations/useResolveUnassignedTransaction', () => ({
  useResolveUnassignedTransaction: () => ({ mutate, isPending: false }),
}));
vi.mock('@/lib/ipc', () => ({
  API: { reconciliation: { dismissUnassigned: (...a: unknown[]) => dismissUnassigned(...a) } },
}));

const record = (over: Partial<UnassignedTransactionRecord> = {}) =>
  ({
    id: 'u1',
    reason: 'extraction_failed',
    merchant_raw: 'Google Cloud',
    amount_minor: 3152,
    currency: 'INR',
    direction: null,
    event_time: '2026-07-29 10:00:00',
    ...over,
  }) as UnassignedTransactionRecord;

beforeEach(() => {
  vi.clearAllMocks();
  dismissUnassigned.mockResolvedValue(undefined);
});

describe('useUnassignedForm', () => {
  it('prefills from the record, converting minor units and trimming the date', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    expect(result.current.fields).toMatchObject({
      merchant: 'Google Cloud',
      amount: '31.52',
      direction: 'debit',
      date: '2026-07-29',
    });
  });

  it('reads a credit direction off the record', () => {
    const { result } = renderHook(() => useUnassignedForm(record({ direction: 'credit' }), onClose));
    expect(result.current.fields.direction).toBe('credit');
  });

  it('is not submittable until an instrument is chosen', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    expect(result.current.canSubmit).toBe(false);

    act(() => result.current.setters.setInstrumentId('inst-1'));
    expect(result.current.canSubmit).toBe(true);
  });

  it('refuses to save while incomplete', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    act(() => result.current.handleSave());
    expect(mutate).not.toHaveBeenCalled();
  });

  it('saves in minor units with a day-start time and a trimmed merchant', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    act(() => {
      result.current.setters.setInstrumentId('inst-1');
      result.current.setters.setMerchant('  Google Cloud  ');
    });
    act(() => result.current.handleSave());

    expect(mutate).toHaveBeenCalledWith(
      expect.objectContaining({
        id: 'u1',
        amountMinor: 3152,
        currency: 'INR',
        direction: 'debit',
        eventTime: '2026-07-29 00:00:00',
        merchantName: 'Google Cloud',
        instrumentId: 'inst-1',
        referenceId: undefined,
      }),
      expect.anything()
    );
  });

  it('defaults the currency when the record carries none', () => {
    const { result } = renderHook(() => useUnassignedForm(record({ currency: null }), onClose));
    act(() => result.current.setters.setInstrumentId('inst-1'));
    act(() => result.current.handleSave());
    expect(mutate).toHaveBeenCalledWith(
      expect.objectContaining({ currency: 'INR' }),
      expect.anything()
    );
  });

  it('dismisses through the IPC, then invalidates and closes', async () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    await act(async () => {
      await result.current.handleDismiss();
    });
    expect(dismissUnassigned).toHaveBeenCalledWith('u1');
    expect(invalidateQueries).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it('does not close when the dismiss fails', async () => {
    dismissUnassigned.mockRejectedValue(new Error('offline'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    await act(async () => {
      await result.current.handleDismiss();
    });
    expect(onClose).not.toHaveBeenCalled();
  });

  it('applies a quick-fill to the named field only', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    act(() => result.current.applyQuickFill({ field: 'amount', value: '99.99' }));
    expect(result.current.fields.amount).toBe('99.99');
    expect(result.current.fields.merchant).toBe('Google Cloud');
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Quick-Fill Applied' }));
  });

  it('ignores a quick-fill for a field it does not own', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    act(() => result.current.applyQuickFill({ field: 'unknown', value: 'x' }));
    expect(result.current.fields.merchant).toBe('Google Cloud');
  });
});
