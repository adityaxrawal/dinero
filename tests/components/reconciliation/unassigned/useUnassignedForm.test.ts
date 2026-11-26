// Saving an unassigned record is the manual escape hatch when extraction
// failed, so its conversion to wire units and its guards are pinned here.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useUnassignedForm } from '@/components/reconciliation/unassigned/useUnassignedForm';
import type { UnassignedTransactionRecord } from '@/lib/ipc';

const mutate = vi.fn();
const toast = vi.fn();
const invalidateQueries = vi.fn();
const dismissUnassigned = vi.fn();
const restoreUnassigned = vi.fn();
const createRule = vi.fn();
const onClose = vi.fn();

vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({ invalidateQueries }) }));
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/lib/errorMapping', () => ({ getErrorToast: () => ({ title: 'failed' }) }));
vi.mock('@/hooks/mutations/useResolveUnassignedTransaction', () => ({
  useResolveUnassignedTransaction: () => ({ mutate, isPending: false }),
}));
vi.mock('@/lib/ipc', () => ({
  API: {
    reconciliation: {
      dismissUnassigned: (...a: unknown[]) => dismissUnassigned(...a),
      restoreUnassigned: (...a: unknown[]) => restoreUnassigned(...a),
    },
    instrumentRules: { create: (...a: unknown[]) => createRule(...a) },
  },
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
  restoreUnassigned.mockResolvedValue(undefined);
  createRule.mockResolvedValue('created');
});

describe('useUnassignedForm', () => {
  it('prefills from the record, converting minor units and trimming the date', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    expect(result.current.fields).toMatchObject({
      merchant: 'Google Cloud',
      amount: '31.52',
      direction: '',
      date: '2026-07-29',
    });
  });

  // The record has no direction: the user must choose rather than get a
  // silent 'debit' (owner decision Q-9).
  it('requires a direction choice when the email gave none', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    act(() => result.current.setters.setInstrumentId('inst-1'));
    expect(result.current.canSubmit).toBe(false);
    act(() => result.current.setters.setDirection('debit'));
    expect(result.current.canSubmit).toBe(true);
  });

  it('reads a credit direction off the record', () => {
    const { result } = renderHook(() =>
      useUnassignedForm(record({ direction: 'credit' }), onClose)
    );
    expect(result.current.fields.direction).toBe('credit');
  });

  it('is not submittable until an instrument is chosen', () => {
    const { result } = renderHook(() => useUnassignedForm(record({ direction: 'debit' }), onClose));
    expect(result.current.canSubmit).toBe(false);

    act(() => result.current.setters.setInstrumentId('inst-1'));
    expect(result.current.canSubmit).toBe(true);
  });

  it('refuses to save while incomplete', () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    act(() => result.current.handleSave());
    expect(mutate).not.toHaveBeenCalled();
  });

  it('sends midnight when the time is cleared (date only)', () => {
    const { result } = renderHook(() => useUnassignedForm(record({ direction: 'debit' }), onClose));
    act(() => {
      result.current.setters.setInstrumentId('inst-1');
      result.current.setters.setTime('');
    });
    act(() => result.current.handleSave());
    expect(mutate).toHaveBeenCalledWith(
      expect.objectContaining({ eventTime: '2026-07-29 00:00:00' }),
      expect.anything()
    );
  });

  it('saves in minor units with the printed time and a trimmed merchant', () => {
    const { result } = renderHook(() => useUnassignedForm(record({ direction: 'debit' }), onClose));
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
        // The email's printed time is pre-filled and kept (audit F9-6).
        eventTime: '2026-07-29 10:00:00',
        merchantName: 'Google Cloud',
        instrumentId: 'inst-1',
        referenceId: undefined,
      }),
      expect.anything()
    );
  });

  it('defaults the currency when the record carries none', () => {
    const { result } = renderHook(() => useUnassignedForm(record({ currency: null, direction: 'debit' }), onClose));
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

  it('does not close and shows an error toast when the dismiss fails', async () => {
    dismissUnassigned.mockRejectedValue(new Error('offline'));
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    await act(async () => {
      await result.current.handleDismiss();
    });
    expect(onClose).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ variant: 'destructive' }));
  });

  it('offers an Undo that restores the dismissed entry', async () => {
    const { result } = renderHook(() => useUnassignedForm(record(), onClose));
    await act(async () => {
      await result.current.handleDismiss();
    });
    const spec = toast.mock.calls.at(-1)?.[0] as { action: { props: { onClick: () => void } } };
    await act(async () => {
      spec.action.props.onClick();
    });
    expect(restoreUnassigned).toHaveBeenCalledWith('u1');
  });

  // Owner decision Q-6: after picking a card for an alert with no card number,
  // the user may confirm a rule for that issuer and card type.
  it('offers "Always use this card" after saving an alert with no card number', async () => {
    const { result } = renderHook(() =>
      useUnassignedForm(
        record({ direction: 'debit', rule_issuer: 'Jupiter', rule_instrument_type: 'credit_card' }),
        onClose
      )
    );
    act(() => result.current.setters.setInstrumentId('inst-1'));
    act(() => result.current.handleSave());
    act(() => mutate.mock.calls[0][1].onSuccess('new_canonical'));
    const spec = toast.mock.calls.at(-1)?.[0] as { action: { props: { onClick: () => void } } };
    await act(async () => spec.action.props.onClick());
    expect(createRule).toHaveBeenCalledWith('Jupiter', 'credit_card', 'inst-1');
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
