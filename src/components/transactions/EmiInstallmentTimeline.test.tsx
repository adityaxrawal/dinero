import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import EmiInstallmentTimeline from './EmiInstallmentTimeline';

let summary: unknown;
let isLoading = false;

vi.mock('@/hooks/queries/useEmiGroup', () => ({
  useEmiGroup: () => ({ data: summary, isLoading }),
}));

const installment = (i: number) => ({
  transaction_id: `tx${i}`,
  event_time: '2026-01-15T00:00:00Z',
  amount_minor: 250000,
});

const group = (over = {}) => ({
  total_installments: 6,
  installments_paid: 2,
  installments: [installment(1), installment(2)],
  ...over,
});

beforeEach(() => {
  isLoading = false;
  summary = group();
});

const renderTimeline = () => render(<EmiInstallmentTimeline emiGroupId="emi1" />);

describe('EmiInstallmentTimeline', () => {
  it('shows a spinner while loading', () => {
    isLoading = true;
    renderTimeline();
    expect(screen.getByRole('status')).toBeTruthy();
  });

  it('renders nothing when the group cannot be found', () => {
    summary = undefined;
    const { container } = renderTimeline();
    expect(container).toBeEmptyDOMElement();
  });

  it('summarises paid against total', () => {
    renderTimeline();
    expect(screen.getByText(/2 of 6 paid/)).toBeTruthy();
  });

  it('reports progress as a percentage', () => {
    renderTimeline();
    expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('33');
  });

  it('lists one row per paid installment, numbered from 1', () => {
    renderTimeline();
    expect(screen.getByText('Installment 1')).toBeTruthy();
    expect(screen.getByText('Installment 2')).toBeTruthy();
  });

  it('renders placeholder slots for the installments not yet seen', () => {
    renderTimeline();
    // 6 total - 2 paid = 4 upcoming, continuing the numbering at 3.
    expect(screen.getAllByText(/upcoming/)).toHaveLength(4);
    expect(screen.getByText('Installment 3 — upcoming')).toBeTruthy();
    expect(screen.getByText('Installment 6 — upcoming')).toBeTruthy();
  });

  it('converts installment amounts out of minor units', () => {
    renderTimeline();
    expect(screen.getAllByText(/₹ 2,500/)[0]).toBeTruthy();
  });

  it('falls back to a dash when an installment has no date', () => {
    summary = group({
      installments: [{ ...installment(1), event_time: null }],
      installments_paid: 1,
    });
    renderTimeline();
    expect(screen.getByText('—')).toBeTruthy();
  });

  describe('when the total is not yet known', () => {
    beforeEach(() => {
      summary = group({ total_installments: null, installments_paid: 2 });
    });

    it('treats the paid count as the total so far', () => {
      renderTimeline();
      expect(screen.getByText(/2 of 2 paid/)).toBeTruthy();
    });

    it('says so explicitly rather than implying the plan is complete', () => {
      renderTimeline();
      expect(screen.getByText(/total unknown/)).toBeTruthy();
    });

    it('shows no upcoming placeholders', () => {
      renderTimeline();
      expect(screen.queryByText(/upcoming/)).toBeNull();
    });
  });

  it('does not render negative placeholders when paid exceeds the total', () => {
    summary = group({ total_installments: 2, installments_paid: 3, installments: [installment(1)] });
    renderTimeline();
    expect(screen.queryByText(/upcoming/)).toBeNull();
  });

  it('avoids dividing by zero on an empty plan', () => {
    summary = group({ total_installments: 0, installments_paid: 0, installments: [] });
    renderTimeline();
    expect(screen.getByRole('progressbar').getAttribute('aria-valuenow')).toBe('0');
  });
});
