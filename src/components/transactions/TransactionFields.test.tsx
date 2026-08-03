import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MerchantField, EmptyTagsNotice, TransactionAuditRows } from './TransactionFields';
import type { CanonicalTransaction } from '@/lib/ipc';

const tx = (over: Partial<CanonicalTransaction> = {}): CanonicalTransaction =>
  ({
    id: 'tx_abc123',
    status: 'posted',
    best_posting_date: null,
    reference_id: null,
    location: null,
    source_mix: null,
    match_confidence: null,
    event_time_confidence: null,
    alert_fired: null,
    ...over,
  }) as CanonicalTransaction;

describe('MerchantField', () => {
  it('shows the current merchant', () => {
    render(<MerchantField id="m" merchant="Swiggy" onChange={vi.fn()} onSubmit={vi.fn()} />);
    expect(screen.getByDisplayValue('Swiggy')).toBeInTheDocument();
  });

  it('reports each edit', () => {
    const onChange = vi.fn();
    render(<MerchantField id="m" merchant="" onChange={onChange} onSubmit={vi.fn()} />);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Zomato' } });
    expect(onChange).toHaveBeenCalledWith('Zomato');
  });

  it('submits on Enter', () => {
    const onSubmit = vi.fn();
    render(<MerchantField id="m" merchant="Swiggy" onChange={vi.fn()} onSubmit={onSubmit} />);
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });
    expect(onSubmit).toHaveBeenCalled();
  });

  it('does not submit on other keys', () => {
    const onSubmit = vi.fn();
    render(<MerchantField id="m" merchant="Swiggy" onChange={vi.fn()} onSubmit={onSubmit} />);
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'a' });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('offers a clear button only when there is something to clear', () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <MerchantField id="m" merchant="" onChange={onChange} onSubmit={vi.fn()} />
    );
    expect(screen.queryByRole('button')).toBeNull();
    rerender(<MerchantField id="m" merchant="Swiggy" onChange={onChange} onSubmit={vi.fn()} />);
    fireEvent.click(screen.getByRole('button'));
    expect(onChange).toHaveBeenCalledWith('');
  });
});

describe('EmptyTagsNotice', () => {
  it('explains the empty state', () => {
    render(<EmptyTagsNotice />);
    expect(screen.getByText('No tags added yet.')).toBeInTheDocument();
  });
});

describe('TransactionAuditRows', () => {
  it('always shows the status and transaction id', () => {
    render(<TransactionAuditRows tx={tx()} />);
    expect(screen.getByText('posted')).toBeInTheDocument();
    expect(screen.getByText('tx_abc123')).toBeInTheDocument();
  });

  it('labels a missing status as unknown', () => {
    render(<TransactionAuditRows tx={tx({ status: null })} />);
    expect(screen.getByText('UNKNOWN')).toBeInTheDocument();
  });

  it('tints a posted status differently from a pending one', () => {
    const { container: posted } = render(<TransactionAuditRows tx={tx({ status: 'posted' })} />);
    const { container: pending } = render(<TransactionAuditRows tx={tx({ status: 'pending' })} />);
    const styleOf = (c: HTMLElement) => c.querySelector('span[style]')!.getAttribute('style');
    expect(styleOf(posted)).not.toBe(styleOf(pending));
  });

  it('matches the posted status case-insensitively', () => {
    const { container: upper } = render(<TransactionAuditRows tx={tx({ status: 'POSTED' })} />);
    const { container: lower } = render(<TransactionAuditRows tx={tx({ status: 'posted' })} />);
    const styleOf = (c: HTMLElement) => c.querySelector('span[style]')!.getAttribute('style');
    expect(styleOf(upper)).toBe(styleOf(lower));
  });

  it.each([
    ['Posting Date', { best_posting_date: '2026-01-16' }],
    ['Reference ID', { reference_id: 'REF123' }],
    ['Location', { location: 'Bangalore' }],
    ['Source Pipeline', { source_mix: 'gmail+statement' }],
    ['Match Confidence', { match_confidence: 'high' }],
    ['Time Confidence', { event_time_confidence: 'exact' }],
  ])('shows the %s row only when populated', (label, over) => {
    const { unmount } = render(<TransactionAuditRows tx={tx()} />);
    expect(screen.queryByText(label)).toBeNull();
    unmount();
    render(<TransactionAuditRows tx={tx(over as Partial<CanonicalTransaction>)} />);
    expect(screen.getByText(label)).toBeInTheDocument();
  });

  it.each([
    [true, 'Yes'],
    [false, 'No'],
  ])('renders alert_fired=%s as %s', (alert_fired, expected) => {
    render(<TransactionAuditRows tx={tx({ alert_fired })} />);
    expect(screen.getByText(expected)).toBeInTheDocument();
  });

  it('hides the alert row when it was never recorded', () => {
    render(<TransactionAuditRows tx={tx({ alert_fired: null })} />);
    expect(screen.queryByText('Alert Sent')).toBeNull();
  });
});
