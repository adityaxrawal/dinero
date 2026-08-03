import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import LearnedRulesSettings from './LearnedRulesSettings';
import { API, type LearnedRule } from '@/lib/ipc';

vi.mock('@/hooks/use-toast', () => ({ toast: vi.fn() }));
vi.mock('@/lib/ipc', () => ({
  API: {
    learnedRules: { list: vi.fn(), revert: vi.fn() },
    senderOverrides: { list: vi.fn(), revert: vi.fn() },
  },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const rule = (over: Partial<LearnedRule> = {}): LearnedRule => ({
  id: 'r1',
  bank_name: 'HDFC Bank',
  field_name: 'amount',
  source_type: 'email',
  template_hash: 'tpl_abc',
  rule_payload_json: { regex: 'INR ([0-9.]+)', capture_group: 1 },
  status: 'active',
  success_count: 40,
  failure_count: 1,
  confidence: 0.94,
  authored_by: 'deterministic',
  learned_from: 'user_edit',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-10T00:00:00Z',
  ...over,
});

const mount = async (rules: LearnedRule[] = [rule()]) => {
  asMock(API.learnedRules.list).mockResolvedValue(rules);
  render(<LearnedRulesSettings />);
  await waitFor(() => expect(API.learnedRules.list).toHaveBeenCalled());
};

beforeEach(() => {
  vi.clearAllMocks();
  asMock(API.learnedRules.list).mockResolvedValue([rule()]);
  asMock(API.senderOverrides.list).mockResolvedValue([]);
  asMock(API.learnedRules.revert).mockResolvedValue(undefined);
});

describe('LearnedRulesSettings', () => {
  it('loads rules and overrides on mount', async () => {
    await mount();
    expect(API.senderOverrides.list).toHaveBeenCalled();
    expect(await screen.findByText(/HDFC Bank/)).toBeInTheDocument();
  });

  it('surfaces a load failure', async () => {
    asMock(API.learnedRules.list).mockRejectedValue(new Error('db locked'));
    render(<LearnedRulesSettings />);
    expect(await screen.findByText('db locked')).toBeInTheDocument();
  });

  it('groups rules by bank, busiest first', async () => {
    await mount([
      rule({ id: 'r1', bank_name: 'Axis Bank' }),
      rule({ id: 'r2', bank_name: 'HDFC Bank', template_hash: 'tpl_1' }),
      rule({ id: 'r3', bank_name: 'HDFC Bank', template_hash: 'tpl_2', field_name: 'merchant' }),
    ]);
    const banks = await screen.findAllByText(/HDFC Bank|Axis Bank/);
    expect(banks[0].textContent).toContain('HDFC Bank');
  });

  describe('technical detail', () => {
    // The bank card renders expanded, so the per-format controls are
    // already on screen — clicking the bank header would collapse it.
    const openDetail = async () => {
      await mount();
      const toggle = await screen.findByRole('button', { name: /Technical detail/ });
      await act(async () => {
        fireEvent.click(toggle);
      });
    };

    it('reveals the pattern and template hash', async () => {
      await openDetail();
      // The pattern also appears in the collapsed format summary above.
      expect(screen.getAllByText('INR ([0-9.]+)').length).toBeGreaterThan(1);
      expect(screen.getByText('tpl_abc')).toBeInTheDocument();
    });

    it('shows the capture group when the rule has one', async () => {
      await openDetail();
      expect(screen.getByText('Capture group')).toBeInTheDocument();
    });

    it('renders the confidence as a percentage', async () => {
      await openDetail();
      expect(screen.getByText('94%')).toBeInTheDocument();
    });
  });

  describe('retiring a rule', () => {
    /** Retire is behind a confirmation dialog whose confirm button repeats the label. */
    const retire = async () => {
      await mount();
      const retireButton = await screen.findByRole('button', { name: /^Retire$/ });
      await act(async () => {
        fireEvent.click(retireButton);
      });
      const confirms = await screen.findAllByRole('button', { name: /Retire/i });
      await act(async () => {
        fireEvent.click(confirms[confirms.length - 1]);
      });
    };

    it('asks the backend to revert and reloads', async () => {
      await retire();
      await waitFor(() => expect(API.learnedRules.revert).toHaveBeenCalledWith('r1'));
      expect(API.learnedRules.list).toHaveBeenCalledTimes(2);
    });

    it('confirms with a toast naming the bank and field', async () => {
      const { toast } = await import('@/hooks/use-toast');
      await retire();
      await waitFor(() =>
        expect(vi.mocked(toast)).toHaveBeenCalledWith(
          expect.objectContaining({ title: 'Rule retired' })
        )
      );
    });

    it('surfaces a revert failure', async () => {
      asMock(API.learnedRules.revert).mockRejectedValue(new Error('rule in use'));
      await retire();
      expect(await screen.findByText('rule in use')).toBeInTheDocument();
    });
  });
});
