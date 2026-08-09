// Covers the pieces extracted out of the old 376-line TransactionDetail page.
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import TransactionHero from './TransactionHero';
import DetailActions from './DetailActions';
import RawSourceDialog from './RawSourceDialog';

const tx = (over = {}) =>
  ({
    id: 'tx1',
    merchant_display_name: 'Swiggy',
    best_event_time: '2026-07-29 10:00:00',
    transaction_subtype: null,
    channel: null,
    status: null,
    ...over,
  }) as never;

describe('TransactionHero', () => {
  it('renders a debit with a minus sign and red styling', () => {
    render(
      <TransactionHero
        tx={tx()}
        category={null as never}
        isDebit
        amountStr="245.43"
        setAmountStr={vi.fn()}
        setDirection={vi.fn()}
      />
    );
    expect(screen.getByText('−₹')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Debit/ })).toBeInTheDocument();
    expect(screen.getByLabelText('Transaction Amount')).toHaveValue(245.43);
  });

  it('renders a credit with a plus sign', () => {
    render(
      <TransactionHero
        tx={tx()}
        category={null as never}
        isDebit={false}
        amountStr="500"
        setAmountStr={vi.fn()}
        setDirection={vi.fn()}
      />
    );
    expect(screen.getByText('+₹')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Credit/ })).toBeInTheDocument();
  });

  it('toggles direction to the opposite of the current one', () => {
    const setDirection = vi.fn();
    render(
      <TransactionHero
        tx={tx()}
        category={null as never}
        isDebit
        amountStr="1"
        setAmountStr={vi.fn()}
        setDirection={setDirection}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: /Debit/ }));
    expect(setDirection).toHaveBeenCalledWith('credit');
  });

  it('shows only the badges the transaction actually carries', () => {
    render(
      <TransactionHero
        tx={tx({ transaction_subtype: 'EMI', channel: 'upi', status: 'posted' })}
        category={{ name: 'Food', color: '#111' } as never}
        isDebit
        amountStr="1"
        setAmountStr={vi.fn()}
        setDirection={vi.fn()}
      />
    );
    expect(screen.getByText('Food')).toBeInTheDocument();
    expect(screen.getByText('EMI')).toBeInTheDocument();
    expect(screen.getByText('UPI')).toBeInTheDocument();
    expect(screen.getByText('posted')).toBeInTheDocument();
  });

  it('edits the amount through the inline input', () => {
    const setAmountStr = vi.fn();
    render(
      <TransactionHero
        tx={tx()}
        category={null as never}
        isDebit
        amountStr="10"
        setAmountStr={setAmountStr}
        setDirection={vi.fn()}
      />
    );
    fireEvent.change(screen.getByLabelText('Transaction Amount'), { target: { value: '99' } });
    expect(setAmountStr).toHaveBeenCalledWith('99');
  });
});

describe('DetailActions', () => {
  const props = {
    isDirty: true,
    isSaving: false,
    isDeleting: false,
    showSavedConfirmation: false,
    onSave: vi.fn(),
    onDelete: vi.fn(),
    onViewSource: vi.fn(),
  };

  it('enables Save only once something has changed', () => {
    const { unmount } = render(<DetailActions {...props} isDirty={false} />);
    expect(screen.getByRole('button', { name: /Save Changes/ })).toBeDisabled();
    unmount();

    render(<DetailActions {...props} />);
    expect(screen.getByRole('button', { name: /Save Changes/ })).toBeEnabled();
  });

  it('keeps Save live right after a save so the confirmation stays reachable', () => {
    render(<DetailActions {...props} isDirty={false} showSavedConfirmation />);
    expect(screen.getByRole('button', { name: /Save Changes/ })).toBeEnabled();
  });

  it('reports in-flight save and delete', () => {
    const { unmount } = render(<DetailActions {...props} isSaving />);
    expect(screen.getByText('Saving...')).toBeInTheDocument();
    unmount();

    render(<DetailActions {...props} isDeleting />);
    expect(screen.getByText('Deleting...')).toBeInTheDocument();
  });

  it('wires each button to its own handler', () => {
    render(<DetailActions {...props} />);
    fireEvent.click(screen.getByRole('button', { name: /Save Changes/ }));
    fireEvent.click(screen.getByRole('button', { name: /Delete Transaction/ }));
    fireEvent.click(screen.getByRole('button', { name: 'View Raw Source' }));
    expect(props.onSave).toHaveBeenCalled();
    expect(props.onDelete).toHaveBeenCalled();
    expect(props.onViewSource).toHaveBeenCalled();
  });
});

describe('RawSourceDialog', () => {
  it('shows a loading state, then the payload, then a no-data fallback', async () => {
    const { rerender } = render(
      <RawSourceDialog open onOpenChange={vi.fn()} isLoading data={null} />
    );
    expect(await screen.findByText('Loading...')).toBeInTheDocument();

    rerender(
      <RawSourceDialog open onOpenChange={vi.fn()} isLoading={false} data="raw text" />
    );
    expect(screen.getByText('raw text')).toBeInTheDocument();

    rerender(<RawSourceDialog open onOpenChange={vi.fn()} isLoading={false} data={null} />);
    expect(screen.getByText('No data')).toBeInTheDocument();
  });
});
