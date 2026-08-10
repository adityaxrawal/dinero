// The three panels the learned-rules settings page composes but that no
// existing spec renders: the field/sort filter bar, the collapsed retired
// list, and the sender-bank override rows.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/react';
import { RuleFilters, RetiredRules, SenderOverrides } from '@/components/settings/learnedRules/RulesPanels';
import type { LearnedRule, SenderBankOverride } from '@/lib/ipc';

vi.mock('@/components/settings/learnedRules/BankCard', () => ({
  default: ({ group }: { group: { bank: string } }) => (
    <div data-testid="bank-card">{group.bank}</div>
  ),
}));

const setFieldFilter = vi.fn();
const setSortMode = vi.fn();

const rules = (over: Record<string, unknown> = {}) =>
  ({
    fieldFilter: 'all',
    sortMode: 'default',
    setFieldFilter,
    setSortMode,
    ...over,
  }) as unknown as Parameters<typeof RuleFilters>[0]['rules'];

const rule = (over: Partial<LearnedRule> = {}): LearnedRule =>
  ({
    id: 'r1',
    bank_name: 'HDFC',
    field: 'merchant',
    created_at: '2026-04-01T00:00:00Z',
    ...over,
  }) as LearnedRule;

const override = (over: Partial<SenderBankOverride> = {}): SenderBankOverride =>
  ({
    id: 'o1',
    domain: 'alerts.hdfcbank.net',
    bank_name: 'HDFC Bank',
    created_at: '2026-04-01T00:00:00Z',
    ...over,
  }) as SenderBankOverride;

beforeEach(() => vi.clearAllMocks());

describe('RuleFilters', () => {
  it('offers every field filter and marks the active one', () => {
    render(<RuleFilters rules={rules({ fieldFilter: 'amount' })} />);

    expect(screen.getByRole('button', { name: 'Everything' })).toBeInTheDocument();
    const active = screen.getByRole('button', { name: 'amount' });
    expect(active.className).toContain('bg-[#064E3B]');
  });

  it('selects a filter on click', () => {
    render(<RuleFilters rules={rules()} />);
    fireEvent.click(screen.getByRole('button', { name: 'amount' }));

    expect(setFieldFilter).toHaveBeenCalledWith('amount');
  });

  it('toggles the sort mode both ways', () => {
    const { unmount } = render(<RuleFilters rules={rules({ sortMode: 'default' })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Sorted by bank size' }));
    expect(setSortMode).toHaveBeenCalledWith('weakest');
    unmount();

    render(<RuleFilters rules={rules({ sortMode: 'weakest' })} />);
    fireEvent.click(screen.getByRole('button', { name: 'Sorted by least reliable' }));
    expect(setSortMode).toHaveBeenCalledWith('default');
  });
});

describe('RetiredRules', () => {
  it('starts collapsed and pluralises the count', () => {
    render(<RetiredRules retired={[rule(), rule({ id: 'r2' })]} revertingId={null} onRetire={vi.fn()} />);

    expect(screen.getByRole('button', { name: /2 retired rules/ })).toBeInTheDocument();
    expect(screen.queryByTestId('bank-card')).not.toBeInTheDocument();
  });

  it('singularises a lone retired rule', () => {
    render(<RetiredRules retired={[rule()]} revertingId={null} onRetire={vi.fn()} />);

    expect(screen.getByRole('button', { name: /1 retired rule$/ })).toBeInTheDocument();
  });

  it('expands and collapses the grouped list', () => {
    render(<RetiredRules retired={[rule()]} revertingId={null} onRetire={vi.fn()} />);
    const toggle = screen.getByRole('button', { name: /retired rule/ });

    fireEvent.click(toggle);
    expect(screen.getByTestId('bank-card')).toHaveTextContent('HDFC');

    fireEvent.click(toggle);
    expect(screen.queryByTestId('bank-card')).not.toBeInTheDocument();
  });
});

describe('SenderOverrides', () => {
  it('explains itself when there are none', () => {
    render(<SenderOverrides overrides={[]} revertingId={null} onRemove={vi.fn()} />);

    expect(screen.getByText(/None yet/)).toBeInTheDocument();
  });

  it('shows the domain to bank mapping for each override', () => {
    render(<SenderOverrides overrides={[override()]} revertingId={null} onRemove={vi.fn()} />);

    expect(screen.getByText('alerts.hdfcbank.net')).toBeInTheDocument();
    expect(screen.getByText('HDFC Bank')).toBeInTheDocument();
  });

  it('removes the override that was clicked', () => {
    const onRemove = vi.fn();
    render(
      <SenderOverrides
        overrides={[override(), override({ id: 'o2', domain: 'icici.com' })]}
        revertingId={null}
        onRemove={onRemove}
      />
    );

    const rows = screen.getAllByRole('button', { name: /Remove/ });
    fireEvent.click(rows[1]);

    expect(onRemove).toHaveBeenCalledTimes(1);
    expect(onRemove).toHaveBeenCalledWith(expect.objectContaining({ id: 'o2' }));
  });

  it('disables only the row currently being reverted', () => {
    render(
      <SenderOverrides
        overrides={[override(), override({ id: 'o2', domain: 'icici.com' })]}
        revertingId="o2"
        onRemove={vi.fn()}
      />
    );

    const buttons = screen.getAllByRole('button', { name: /Remove/ });
    expect(buttons[0]).toBeEnabled();
    expect(buttons[1]).toBeDisabled();
  });

  it('keeps each row scoped to its own domain', () => {
    render(
      <SenderOverrides
        overrides={[override(), override({ id: 'o2', domain: 'icici.com', bank_name: 'ICICI' })]}
        revertingId={null}
        onRemove={vi.fn()}
      />
    );

    const icici = screen.getByText('icici.com').closest('div.p-3\\.5') as HTMLElement;
    expect(within(icici).getByText('ICICI')).toBeInTheDocument();
  });
});
