// The Statement Instrument Gate: asked only when the parser could not work
// out which account a statement belongs to.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { renderHook, act } from '@testing-library/react';
import InstrumentGateDialog from '@/pages/statements/InstrumentGateDialog';
import { useInstrumentGate } from '@/pages/statements/useInstrumentGate';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

vi.mock('@/lib/GlobalStateContext', () => ({ useGlobalState: vi.fn() }));
vi.mock('@/lib/ipc', () => ({ API: { statements: { confirmInstrument: vi.fn() } } }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const closeInstrumentModal = vi.fn();
const openReviewModal = vi.fn();
const refresh = vi.fn();

function mockState(over: Record<string, unknown> = {}) {
  asMock(useGlobalState).mockImplementation(() => ({
    instrumentModalOpen: true,
    pendingInstrumentStatementId: 's1',
    pendingInstrumentFilename: 'june.pdf',
    pendingInstrumentIssuerHint: 'HDFC',
    pendingInstrumentReason: 'Issuer unreadable',
    closeInstrumentModal,
    openReviewModal,
    ...over,
  }));
}

beforeEach(() => {
  vi.clearAllMocks();
  mockState();
});

function Harness() {
  const gate = useInstrumentGate(refresh);
  return <InstrumentGateDialog gate={gate} />;
}

describe('InstrumentGateDialog', () => {
  it('names the file and the reason it is asking', async () => {
    render(<Harness />);
    expect(await screen.findByText(/june\.pdf: Issuer unreadable/)).toBeInTheDocument();
  });

  it('prefills the issuer from the parser hint', async () => {
    render(<Harness />);
    expect(await screen.findByLabelText('Issuer / Bank Name')).toHaveValue('HDFC');
  });

  it('falls back to generic copy when no reason was given', async () => {
    mockState({ pendingInstrumentReason: '', pendingInstrumentFilename: '' });
    render(<Harness />);
    expect(
      await screen.findByText(/could not automatically identify the issuer/)
    ).toBeInTheDocument();
  });

  it('keeps Confirm disabled until both issuer and last-4 are filled', async () => {
    mockState({ pendingInstrumentIssuerHint: '' });
    render(<Harness />);
    const confirm = await screen.findByRole('button', {
      name: 'Confirm statement instrument details',
    });
    expect(confirm).toBeDisabled();

    fireEvent.change(screen.getByLabelText('Issuer / Bank Name'), { target: { value: 'Axis' } });
    expect(confirm).toBeDisabled();

    fireEvent.change(screen.getByLabelText(/Last 4 Digits/), { target: { value: '4321' } });
    expect(confirm).toBeEnabled();
  });

  it('strips non-digits out of the last-4 field', async () => {
    render(<Harness />);
    const masked = await screen.findByLabelText(/Last 4 Digits/);
    fireEvent.change(masked, { target: { value: 'ab12cd34' } });
    expect(masked).toHaveValue('1234');
  });

  it('cancels without confirming anything', async () => {
    render(<Harness />);
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel instrument confirmation' }));
    expect(closeInstrumentModal).toHaveBeenCalled();
    expect(API.statements.confirmInstrument).not.toHaveBeenCalled();
  });
});

describe('useInstrumentGate.submit', () => {
  it('refuses to submit while a required field is blank', async () => {
    const { result } = renderHook(() => useInstrumentGate(refresh));
    await act(async () => {
      await result.current.submit();
    });
    expect(API.statements.confirmInstrument).not.toHaveBeenCalled();
  });

  it('confirms with trimmed values, then opens review on the returned draft', async () => {
    asMock(API.statements.confirmInstrument).mockResolvedValue({ draft_id: 'draft-3' });
    const { result } = renderHook(() => useInstrumentGate(refresh));

    act(() => {
      result.current.setIssuer('  Axis  ');
      result.current.setMasked(' 4321 ');
    });
    await act(async () => {
      await result.current.submit();
    });

    expect(API.statements.confirmInstrument).toHaveBeenCalledWith(
      's1',
      'Axis',
      '4321',
      'credit_card'
    );
    expect(openReviewModal).toHaveBeenCalledWith('draft-3');
    expect(refresh).toHaveBeenCalled();
  });

  it('falls back to the statement id when no draft id comes back', async () => {
    asMock(API.statements.confirmInstrument).mockResolvedValue({ draft_id: null });
    const { result } = renderHook(() => useInstrumentGate(refresh));
    act(() => {
      result.current.setIssuer('Axis');
      result.current.setMasked('4321');
    });
    await act(async () => {
      await result.current.submit();
    });
    expect(openReviewModal).toHaveBeenCalledWith('s1');
  });

  it('surfaces a confirmation failure inline instead of closing', async () => {
    asMock(API.statements.confirmInstrument).mockRejectedValue(new Error('parser gave up'));
    const { result } = renderHook(() => useInstrumentGate(refresh));
    act(() => {
      result.current.setIssuer('Axis');
      result.current.setMasked('4321');
    });
    await act(async () => {
      await result.current.submit();
    });

    await waitFor(() => expect(result.current.error).toBeTruthy());
    expect(closeInstrumentModal).not.toHaveBeenCalled();
  });
});
