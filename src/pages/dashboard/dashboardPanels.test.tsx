// Covers the panels extracted out of the old 278-line Dashboard component.
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import KpiRow from './KpiRow';
import AttentionRail from './AttentionRail';
import ChartsRow from './ChartsRow';
import type { useDashboardData } from './useDashboardData';

const navigate = vi.fn();
vi.mock('react-router-dom', () => ({ useNavigate: () => navigate }));
vi.mock('@/components/dashboard/classifyBillUrgency', () => ({
  classifyBillUrgency: (due: string) => (due === 'overdue' ? 'overdue' : 'critical'),
}));
// Recharts needs a sized container jsdom never provides; the charts themselves
// are covered by their own tests.
vi.mock('./charts', () => ({
  TrendChart: () => <div data-testid="trend-chart" />,
  CategoryDonut: ({ onSliceClick }: { onSliceClick: (id: string) => void }) => (
    <button onClick={() => onSliceClick('cat-1')}>donut</button>
  ),
}));

type Data = ReturnType<typeof useDashboardData>;

const data = (over: Partial<Data> = {}): Data =>
  ({
    granularity: 'daily',
    setGranularity: vi.fn(),
    summary: null,
    loading: false,
    transactions: [],
    trendData: [],
    trendLoading: false,
    categorySlices: [],
    categoriesLoading: false,
    delta: null,
    pending: null,
    urgentBills: [],
    clusters: [],
    hasAttentionItems: false,
    ...over,
  }) as unknown as Data;

describe('KpiRow', () => {
  it('reports spend, income and a positive net', () => {
    render(<KpiRow spend={1200} income={5000} limit={0} delta={null} />);
    expect(screen.getByText('₹1,200')).toBeInTheDocument();
    expect(screen.getByText('₹5,000')).toBeInTheDocument();
    expect(screen.getByText('+₹3,800')).toBeInTheDocument();
  });

  it('shows an overspent net without a plus sign', () => {
    render(<KpiRow spend={5000} income={1200} limit={0} delta={null} />);
    expect(screen.getByText('₹3,800')).toBeInTheDocument();
    expect(screen.queryByText('+₹3,800')).not.toBeInTheDocument();
  });

  it('shows the month-over-month delta when one is known', () => {
    render(<KpiRow spend={100} income={0} limit={0} delta={12.34} />);
    expect(screen.getByText(/12\.3% vs last month/)).toBeInTheDocument();
  });

  it('adds a limit gauge only when a limit is set', () => {
    const { unmount } = render(<KpiRow spend={50} income={0} limit={0} delta={null} />);
    expect(screen.queryByText('Monthly Limit')).not.toBeInTheDocument();
    unmount();

    render(<KpiRow spend={50} income={0} limit={200} delta={null} />);
    expect(screen.getByText('Monthly Limit')).toBeInTheDocument();
    expect(screen.getByText('25%')).toBeInTheDocument();
  });
});

describe('AttentionRail', () => {
  it('links pending reviews to the reconciliation queue', () => {
    render(<AttentionRail data={data({ pending: { count: 3, amount_minor: 45000 } as never })} />);
    expect(screen.getByText('3 Pending Review')).toBeInTheDocument();

    fireEvent.click(screen.getByText('3 Pending Review'));
    expect(navigate).toHaveBeenCalledWith('/reconciliation');
  });

  it('distinguishes an overdue bill from one merely due soon', () => {
    render(
      <AttentionRail
        data={data({
          urgentBills: [
            { id: 'b1', description: 'Card bill', amount: 900, due_date: 'overdue' },
            { id: 'b2', description: 'Loan', amount: 100, due_date: '2026-09-01' },
          ] as never,
        })}
      />
    );
    expect(screen.getByText('Overdue')).toBeInTheDocument();
    expect(screen.getByText('Due soon')).toBeInTheDocument();
  });

  it('pluralises the unresolved cluster count', () => {
    render(<AttentionRail data={data({ clusters: [{ id: 'c1' }, { id: 'c2' }] as never })} />);
    expect(screen.getByText('2 Unresolved Clusters')).toBeInTheDocument();
  });

  it('falls back to upcoming bills only when nothing is urgent', () => {
    render(
      <AttentionRail data={data({ summary: { upcoming_bills_count: 1 } as never })} />
    );
    expect(screen.getByText('1 Upcoming Bill')).toBeInTheDocument();
  });
});

describe('ChartsRow', () => {
  it('spins while either chart is still loading', () => {
    const { container } = render(
      <ChartsRow data={data({ trendLoading: true, categoriesLoading: true })} onCategoryClick={vi.fn()} />
    );
    expect(container.querySelectorAll('.animate-spin').length).toBe(2);
  });

  it('routes a donut click and a legend click through the same handler', () => {
    const onCategoryClick = vi.fn();
    render(
      <ChartsRow
        data={data({
          categorySlices: [
            { category_id: 'cat-1', name: 'Food', total_spend: 500, color: '#000' },
          ] as never,
        })}
        onCategoryClick={onCategoryClick}
      />
    );
    fireEvent.click(screen.getByText('donut'));
    fireEvent.click(screen.getByText('Food'));
    expect(onCategoryClick).toHaveBeenCalledTimes(2);
    expect(onCategoryClick).toHaveBeenCalledWith('cat-1');
  });

  it('offers all three granularities and reports the chosen one', () => {
    const setGranularity = vi.fn();
    render(<ChartsRow data={data({ setGranularity })} onCategoryClick={vi.fn()} />);
    fireEvent.click(screen.getByText('Wee'));
    expect(setGranularity).toHaveBeenCalledWith('weekly');
  });
});
