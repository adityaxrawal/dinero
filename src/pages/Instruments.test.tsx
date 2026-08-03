import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import Instruments from './Instruments';
import type { InstrumentRecord } from '@/lib/ipc';

let instruments: InstrumentRecord[] = [];
let isLoading = false;

vi.mock('@/hooks/queries/useInstrumentsList', () => ({
  useInstrumentsList: () => ({ data: instruments, isLoading }),
}));
vi.mock('@/components/instruments/AddInstrumentModal', () => ({
  default: ({ open }: { open: boolean }) => (open ? <div data-testid="add-modal" /> : null),
}));
vi.mock('@/components/instruments/InstrumentInspector', () => ({
  default: ({ instrument }: { instrument?: InstrumentRecord }) => (
    <div data-testid="inspector">{instrument?.id ?? 'none'}</div>
  ),
}));

const inst = (over: Partial<InstrumentRecord> = {}): InstrumentRecord => ({
  id: 'i1',
  instrument_type: 'credit_card',
  issuer_name: 'HDFC Bank',
  masked_identifier: '8841',
  status: 'active',
  current_balance: -1500,
  credit_limit: 100000,
  ...over,
});

const card = inst();
const bank = inst({
  id: 'i2',
  instrument_type: 'bank_account',
  issuer_name: 'Axis Bank',
  masked_identifier: '2210',
  current_balance: 25000,
});
const upi = inst({
  id: 'i3',
  instrument_type: 'upi_vpa',
  issuer_name: 'Jupiter',
  masked_identifier: 'me@jupiter',
  current_balance: 0,
});

const pill = (name: RegExp) => screen.getByRole('button', { name });
const search = () => screen.getByPlaceholderText('Search accounts, cards...');

beforeEach(() => {
  isLoading = false;
  instruments = [card, bank, upi];
});

describe('Instruments', () => {
  it('shows a loading state', () => {
    isLoading = true;
    instruments = [];
    render(<Instruments />);
    expect(screen.getByText(/Loading accounts/)).toBeInTheDocument();
  });

  it('invites the user to add one when there are none', () => {
    instruments = [];
    render(<Instruments />);
    expect(screen.getByText(/No instruments yet/)).toBeInTheDocument();
  });

  it('opens the add modal from the empty state', () => {
    instruments = [];
    render(<Instruments />);
    fireEvent.click(screen.getByRole('button', { name: /Add a new account/ }));
    expect(screen.getByTestId('add-modal')).toBeInTheDocument();
  });

  it('opens the add modal from the header', () => {
    render(<Instruments />);
    fireEvent.click(screen.getByLabelText('Add account'));
    expect(screen.getByTestId('add-modal')).toBeInTheDocument();
  });

  it('lists every instrument grouped by type', () => {
    render(<Instruments />);
    expect(screen.getByText('HDFC Bank')).toBeInTheDocument();
    expect(screen.getByText('Axis Bank')).toBeInTheDocument();
    expect(screen.getByText('Jupiter')).toBeInTheDocument();
  });

  it('auto-selects the first instrument', () => {
    render(<Instruments />);
    expect(screen.getByTestId('inspector').textContent).toBe('i1');
  });

  it('selects a different instrument on click', () => {
    render(<Instruments />);
    fireEvent.click(screen.getByText('Axis Bank'));
    expect(screen.getByTestId('inspector').textContent).toBe('i2');
  });

  it('falls back to a generic name for an unnamed issuer', () => {
    instruments = [inst({ issuer_name: '' })];
    render(<Instruments />);
    expect(screen.getByText('Account')).toBeInTheDocument();
  });

  describe('category pills', () => {
    it('counts each category', () => {
      render(<Instruments />);
      expect(pill(/All/).textContent).toContain('3');
      expect(pill(/Cards/).textContent).toContain('1');
      expect(pill(/Banks/).textContent).toContain('1');
      expect(pill(/UPI/).textContent).toContain('1');
    });

    it('filters down to one type', () => {
      render(<Instruments />);
      fireEvent.click(pill(/Cards/));
      expect(screen.getByText('HDFC Bank')).toBeInTheDocument();
      expect(screen.queryByText('Axis Bank')).toBeNull();
    });

    it('restores everything via All', () => {
      render(<Instruments />);
      fireEvent.click(pill(/Cards/));
      fireEvent.click(pill(/All/));
      expect(screen.getByText('Axis Bank')).toBeInTheDocument();
    });
  });

  describe('search', () => {
    it('matches on issuer name, case-insensitively', () => {
      render(<Instruments />);
      fireEvent.change(search(), { target: { value: 'axis' } });
      expect(screen.getByText('Axis Bank')).toBeInTheDocument();
      expect(screen.queryByText('HDFC Bank')).toBeNull();
    });

    it('matches on the masked identifier', () => {
      render(<Instruments />);
      fireEvent.change(search(), { target: { value: '8841' } });
      expect(screen.getByText('HDFC Bank')).toBeInTheDocument();
      expect(screen.queryByText('Axis Bank')).toBeNull();
    });

    it('matches on instrument type', () => {
      render(<Instruments />);
      fireEvent.change(search(), { target: { value: 'upi' } });
      expect(screen.getByText('Jupiter')).toBeInTheDocument();
      expect(screen.queryByText('HDFC Bank')).toBeNull();
    });

    it('ignores surrounding whitespace', () => {
      render(<Instruments />);
      fireEvent.change(search(), { target: { value: '  axis  ' } });
      expect(screen.getByText('Axis Bank')).toBeInTheDocument();
    });

    it('reports no matches', () => {
      render(<Instruments />);
      fireEvent.change(search(), { target: { value: 'nonexistent' } });
      expect(screen.getByText(/none match your criteria/)).toBeInTheDocument();
    });

    it('combines with the category filter', () => {
      render(<Instruments />);
      fireEvent.click(pill(/Cards/));
      fireEvent.change(search(), { target: { value: 'axis' } });
      expect(screen.getByText(/none match your criteria/)).toBeInTheDocument();
    });
  });
});
