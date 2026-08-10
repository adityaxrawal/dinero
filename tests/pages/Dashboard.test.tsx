// The Dashboard shell: the loading gate, the conditional attention rail, the
// time-of-day greeting, and the category drill-through. The panels themselves
// are mocked -- each has its own spec.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import Dashboard from '@/pages/Dashboard';
import { useDashboardData } from '@/pages/dashboard/useDashboardData';

const navigate = vi.fn();
vi.mock('react-router-dom', () => ({ useNavigate: () => navigate }));
vi.mock('@/pages/dashboard/useDashboardData', () => ({ useDashboardData: vi.fn() }));

vi.mock('@/pages/dashboard/KpiRow', () => ({
  default: ({ spend }: { spend: number }) => <div data-testid="kpi-row">spend:{spend}</div>,
}));
vi.mock('@/pages/dashboard/AttentionRail', () => ({
  default: () => <div data-testid="attention-rail" />,
}));
vi.mock('@/pages/dashboard/ChartsRow', () => ({
  default: ({ onCategoryClick }: { onCategoryClick: (id: string) => void }) => (
    <div data-testid="charts-row">
      <button onClick={() => onCategoryClick('food & drink')}>pick category</button>
      <button onClick={() => onCategoryClick('__other__')}>pick other</button>
    </div>
  ),
}));
vi.mock('@/components/dashboard/StaleClusterReminder', () => ({ default: () => null }));
vi.mock('@/components/dashboard/RecentTransactions', () => ({ default: () => null }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

function setData(over: Record<string, unknown> = {}) {
  asMock(useDashboardData).mockReturnValue({
    loading: false,
    summary: { month_to_date_spend: 4200, income: 90000, limit: 50000 },
    delta: 12,
    hasAttentionItems: false,
    clusters: [],
    transactions: [],
    ...over,
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  setData();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('Dashboard', () => {
  it('shows a labelled spinner while data loads', () => {
    setData({ loading: true });
    render(<Dashboard />);

    expect(screen.getByRole('status', { name: 'Loading dashboard' })).toBeInTheDocument();
    expect(screen.queryByTestId('kpi-row')).not.toBeInTheDocument();
  });

  it('keeps the spinner when loading finishes without a summary', () => {
    // Both guards matter: a settled query with no summary must not fall
    // through to KpiRow, which dereferences it.
    setData({ loading: false, summary: null });
    render(<Dashboard />);

    expect(screen.getByRole('status', { name: 'Loading dashboard' })).toBeInTheDocument();
    expect(screen.queryByTestId('kpi-row')).not.toBeInTheDocument();
  });

  it('renders the panels once a summary is available', () => {
    render(<Dashboard />);

    expect(screen.getByTestId('kpi-row')).toHaveTextContent('spend:4200');
    expect(screen.getByTestId('charts-row')).toBeInTheDocument();
    expect(screen.queryByRole('status', { name: 'Loading dashboard' })).not.toBeInTheDocument();
  });

  it('shows the attention rail only when there is something to attend to', () => {
    render(<Dashboard />);
    expect(screen.queryByTestId('attention-rail')).not.toBeInTheDocument();

    setData({ hasAttentionItems: true });
    render(<Dashboard />);
    expect(screen.getAllByTestId('attention-rail')).toHaveLength(1);
  });

  it('drills into a category with the id URL-encoded', () => {
    render(<Dashboard />);
    fireEvent.click(screen.getByText('pick category'));

    expect(navigate).toHaveBeenCalledWith('/transactions?category=food%20%26%20drink');
  });

  it('does not drill into the synthetic "other" bucket', () => {
    // `__other__` is an aggregate of the long tail, not a real category, so
    // there is no transaction filter that corresponds to it.
    render(<Dashboard />);
    fireEvent.click(screen.getByText('pick other'));

    expect(navigate).not.toHaveBeenCalled();
  });

  it.each([
    ['2026-08-09T08:00:00', 'Good morning'],
    ['2026-08-09T13:00:00', 'Good afternoon'],
    ['2026-08-09T20:00:00', 'Good evening'],
  ])('greets appropriately at %s', (now, greeting) => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(now));
    render(<Dashboard />);

    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent(greeting);
  });
});
