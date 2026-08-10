import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useInstrumentForm } from '@/components/instruments/useInstrumentForm';
import { API, type InstrumentRecord } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';

const toast = vi.fn();
const invalidateQueries = vi.fn();

let detailInst: InstrumentRecord | undefined;
let txData: unknown;
let statements: Array<{ id: string; instrument_id: string }> = [];
let pdfPasswords: Array<{ id: string; instrument_id: string }> = [];

vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@tanstack/react-query', () => ({ useQueryClient: () => ({ invalidateQueries }) }));
vi.mock('@/lib/confirmDialog', () => ({ confirmAction: vi.fn() }));
vi.mock('@/lib/ipc', () => ({
  API: { instruments: { update: vi.fn(), delete: vi.fn() } },
}));
vi.mock('@/lib/queryKeys', () => ({
  queryKeys: { instruments: { detail: (id: string) => ['inst', id], all: () => ['inst'] } },
}));
vi.mock('@/hooks/queries/useInstrumentDetail', () => ({
  useInstrumentDetail: () => ({ data: detailInst, isLoading: false }),
}));
vi.mock('@/hooks/queries/useTransactionsInfiniteList', () => ({
  useTransactionsInfiniteList: () => ({
    data: txData,
    fetchNextPage: vi.fn(),
    hasNextPage: false,
    isFetchingNextPage: false,
    isLoading: false,
  }),
}));
vi.mock('@/hooks/queries/useStatementsList', () => ({ useStatementsList: () => ({ data: statements }) }));
vi.mock('@/hooks/queries/usePdfPasswordsList', () => ({
  usePdfPasswordsList: () => ({ data: pdfPasswords }),
}));
vi.mock('@/hooks/mutations/useForgetPdfPassword', () => ({
  useForgetPdfPassword: () => ({ mutate: vi.fn() }),
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const instrument = (over: Partial<InstrumentRecord> = {}): InstrumentRecord => ({
  id: 'inst1',
  instrument_type: 'credit_card',
  issuer_name: 'HDFC Bank',
  masked_identifier: '8841',
  status: 'active',
  current_balance: -1500,
  ...over,
});

const setup = (initial?: InstrumentRecord, onClose?: () => void) =>
  renderHook(() => useInstrumentForm('inst1', initial, onClose));

beforeEach(() => {
  vi.clearAllMocks();
  detailInst = instrument();
  txData = { pages: [{ records: [{ id: 'tx1' }], total: 12 }] };
  statements = [];
  pdfPasswords = [];
  asMock(confirmAction).mockResolvedValue(true);
  asMock(API.instruments.update).mockResolvedValue(undefined);
  asMock(API.instruments.delete).mockResolvedValue(undefined);
});

describe('hydration', () => {
  it('seeds the form from the fetched detail', () => {
    detailInst = instrument({ nickname: 'Daily card', credit_limit: 200000, bank_ifsc: 'HDFC0001' });
    const { result } = setup();
    expect(result.current.fields).toMatchObject({
      issuerName: 'HDFC Bank',
      maskedIdentifier: '8841',
      nickname: 'Daily card',
      creditLimit: '200000',
      bankIfsc: 'HDFC0001',
      instrumentType: 'credit_card',
      status: 'active',
    });
  });

  it('falls back to the passed-in instrument before the detail loads', () => {
    detailInst = undefined;
    const { result } = setup(instrument({ issuer_name: 'Axis Bank' }));
    expect(result.current.fields.issuerName).toBe('Axis Bank');
  });

  it('prefers the fetched detail over the passed-in instrument', () => {
    detailInst = instrument({ issuer_name: 'Fresh from DB' });
    const { result } = setup(instrument({ issuer_name: 'Stale list copy' }));
    expect(result.current.fields.issuerName).toBe('Fresh from DB');
  });

  it('converts minimum_due from minor units for display', () => {
    detailInst = instrument({ minimum_due: 250050 });
    expect(setup().result.current.fields.minimumDue).toBe('2500.5');
  });

  it('shows a zero minimum_due rather than treating it as absent', () => {
    detailInst = instrument({ minimum_due: 0 });
    expect(setup().result.current.fields.minimumDue).toBe('0');
  });

  it('blanks optional fields that are absent', () => {
    const { result } = setup();
    expect(result.current.fields).toMatchObject({
      nickname: '',
      fullIdentifier: '',
      billingCycleDay: '',
      creditLimit: '',
      minimumDue: '',
      upiVpa: '',
    });
  });

  it('defaults type and status when the record omits them entirely', () => {
    // `??`, not `||` — a record missing the column falls back, but an
    // explicitly empty string is preserved as the user's own value.
    detailInst = instrument({
      instrument_type: undefined as unknown as string,
      status: undefined as unknown as string,
    });
    const { result } = setup();
    expect(result.current.fields.instrumentType).toBe('credit_card');
    expect(result.current.fields.status).toBe('active');
  });

  it('flags a negative balance', () => {
    expect(setup().result.current.isNegative).toBe(true);
    detailInst = instrument({ current_balance: 500 });
    expect(setup().result.current.isNegative).toBe(false);
  });
});

describe('related records', () => {
  it('flattens paginated transactions and reports the server total', () => {
    txData = { pages: [{ records: [{ id: 'a' }], total: 12 }, { records: [{ id: 'b' }] }] };
    const { result } = setup();
    expect(result.current.recentTransactions).toHaveLength(2);
    expect(result.current.totalTxCount).toBe(12);
  });

  it('falls back to the loaded count when no server total is given', () => {
    txData = { pages: [{ records: [{ id: 'a' }, { id: 'b' }] }] };
    expect(setup().result.current.totalTxCount).toBe(2);
  });

  it('handles the pre-load state', () => {
    txData = undefined;
    const { result } = setup();
    expect(result.current.recentTransactions).toEqual([]);
    expect(result.current.totalTxCount).toBe(0);
  });

  it('shows only this instrument’s statements and passwords', () => {
    statements = [{ id: 's1', instrument_id: 'inst1' }, { id: 's2', instrument_id: 'other' }];
    pdfPasswords = [{ id: 'p1', instrument_id: 'other' }];
    const { result } = setup();
    expect(result.current.instrumentStatements).toHaveLength(1);
    expect(result.current.instrumentPasswords).toHaveLength(0);
  });

  it('returns nothing related when no instrument is resolved', () => {
    detailInst = undefined;
    statements = [{ id: 's1', instrument_id: 'inst1' }];
    const { result } = renderHook(() => useInstrumentForm(undefined));
    expect(result.current.instrumentStatements).toEqual([]);
  });
});

describe('handleSave', () => {
  it('does nothing when no instrument is loaded', async () => {
    detailInst = undefined;
    const { result } = renderHook(() => useInstrumentForm(undefined));
    await act(async () => result.current.handleSave());
    expect(API.instruments.update).not.toHaveBeenCalled();
  });

  it('sends only the fields that carry a value', async () => {
    const { result } = setup();
    act(() => result.current.setField('nickname', 'Daily card'));
    await act(async () => result.current.handleSave());
    const extra = asMock(API.instruments.update).mock.calls[0][4];
    expect(extra.nickname).toBe('Daily card');
    expect(extra).not.toHaveProperty('upi_vpa');
    expect(extra).not.toHaveProperty('credit_limit');
  });

  it('parses numeric fields', async () => {
    const { result } = setup();
    act(() => {
      result.current.setField('creditLimit', '200000');
      result.current.setField('minimumDue', '2500.50');
      result.current.setField('billingCycleDay', '15');
    });
    await act(async () => result.current.handleSave());
    const call = asMock(API.instruments.update).mock.calls[0];
    expect(call[2]).toBe(15);
    expect(call[4]).toMatchObject({ credit_limit: 200000, minimum_due: 2500.5 });
  });

  it('sends undefined for blank optional top-level args', async () => {
    const { result } = setup();
    await act(async () => result.current.handleSave());
    const call = asMock(API.instruments.update).mock.calls[0];
    expect(call[1]).toBeUndefined();
    expect(call[2]).toBeUndefined();
    expect(call[3]).toBeUndefined();
  });

  it('shows a transient saved confirmation and refreshes the caches', async () => {
    vi.useFakeTimers();
    const { result } = setup();
    await act(async () => result.current.handleSave());
    expect(result.current.showSavedConfirm).toBe(true);
    expect(invalidateQueries).toHaveBeenCalledTimes(2);
    act(() => vi.advanceTimersByTime(3000));
    expect(result.current.showSavedConfirm).toBe(false);
    vi.useRealTimers();
  });

  it('clears the saving flag and toasts on failure', async () => {
    asMock(API.instruments.update).mockRejectedValue(new Error('db locked'));
    const { result } = setup();
    await act(async () => result.current.handleSave());
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ variant: 'destructive', description: 'db locked' })
    );
    expect(result.current.isSaving).toBe(false);
    expect(result.current.showSavedConfirm).toBe(false);
  });
});

describe('handleDelete', () => {
  it('does nothing when no instrument is loaded', async () => {
    detailInst = undefined;
    const { result } = renderHook(() => useInstrumentForm(undefined));
    await act(async () => result.current.handleDelete());
    expect(confirmAction).not.toHaveBeenCalled();
  });

  it('names the instrument in the confirmation prompt', async () => {
    const { result } = setup();
    await act(async () => result.current.handleDelete());
    expect(confirmAction).toHaveBeenCalledWith(
      expect.stringContaining('8841'),
      'Delete Instrument'
    );
  });

  it('aborts when the user declines', async () => {
    asMock(confirmAction).mockResolvedValue(false);
    const { result } = setup();
    await act(async () => result.current.handleDelete());
    expect(API.instruments.delete).not.toHaveBeenCalled();
  });

  it('deletes, notifies, closes the panel and refreshes the list', async () => {
    const onClose = vi.fn();
    const { result } = setup(undefined, onClose);
    await act(async () => result.current.handleDelete());
    await waitFor(() => expect(API.instruments.delete).toHaveBeenCalledWith('inst1'));
    expect(toast).toHaveBeenCalledWith({ title: 'Instrument deleted' });
    expect(onClose).toHaveBeenCalled();
    expect(invalidateQueries).toHaveBeenCalled();
  });

  it('survives having no onClose callback', async () => {
    const { result } = setup();
    await expect(act(async () => result.current.handleDelete())).resolves.toBeUndefined();
  });

  it('clears the deleting flag and toasts on failure', async () => {
    asMock(API.instruments.delete).mockRejectedValue(new Error('has transactions'));
    const { result } = setup();
    await act(async () => result.current.handleDelete());
    expect(toast).toHaveBeenCalledWith(
      expect.objectContaining({ variant: 'destructive', description: 'has transactions' })
    );
    expect(result.current.isDeleting).toBe(false);
  });
});
