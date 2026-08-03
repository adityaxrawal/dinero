import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import BudgetsSettings from './BudgetsSettings';
import { API } from '@/lib/ipc';

const toast = vi.fn();
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }) }));
vi.mock('@/lib/ipc', () => ({
  API: { spendingLimits: { get: vi.fn(), update: vi.fn() } },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const limits = (over = {}) => ({
  global_limit: 60000,
  thresholds: { warn_at_80: true, warn_at_90: true, warn_at_100: true },
  categories: [{ name: 'Food', budget: 8000 }],
  ...over,
});

const mount = async () => {
  render(<BudgetsSettings />);
  await waitFor(() => expect(screen.getByLabelText('₹ Limit')).toBeInTheDocument());
};

beforeEach(() => {
  vi.clearAllMocks();
  asMock(API.spendingLimits.get).mockResolvedValue(limits());
  asMock(API.spendingLimits.update).mockResolvedValue(undefined);
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

describe('BudgetsSettings loading', () => {
  it('loads the saved limits on mount', async () => {
    await mount();
    expect(API.spendingLimits.get).toHaveBeenCalled();
    expect(screen.getByLabelText('₹ Limit')).toHaveProperty('value', '60000');
  });

  it('leaves the field blank when no limit is set', async () => {
    asMock(API.spendingLimits.get).mockResolvedValue(limits({ global_limit: 0 }));
    await mount();
    expect(screen.getByLabelText('₹ Limit')).toHaveProperty('value', '');
  });

  it('survives a load failure rather than hanging on the spinner', async () => {
    asMock(API.spendingLimits.get).mockRejectedValue(new Error('db locked'));
    render(<BudgetsSettings />);
    await waitFor(() => expect(screen.getByLabelText('₹ Limit')).toBeInTheDocument());
  });
});

describe('alert thresholds', () => {
  it('shows each threshold as on by default', async () => {
    await mount();
    expect(screen.getAllByText('ON')).toHaveLength(3);
  });

  it('toggles a threshold off', async () => {
    await mount();
    fireEvent.click(screen.getByText('80%').closest('button')!);
    expect(screen.getAllByText('ON')).toHaveLength(2);
    expect(screen.getByText('OFF')).toBeInTheDocument();
  });

  it('reflects a threshold that was saved off', async () => {
    asMock(API.spendingLimits.get).mockResolvedValue(
      limits({ thresholds: { warn_at_80: false, warn_at_90: true, warn_at_100: true } })
    );
    await mount();
    expect(screen.getAllByText('ON')).toHaveLength(2);
  });

  it('sends the toggled thresholds on save', async () => {
    await mount();
    fireEvent.click(screen.getByText('90%').closest('button')!);
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(API.spendingLimits.update).toHaveBeenCalledWith(
        expect.objectContaining({
          thresholds: { warn_at_80: true, warn_at_90: false, warn_at_100: true },
        })
      )
    );
  });
});

describe('saving', () => {
  it('persists the global limit as a number', async () => {
    await mount();
    fireEvent.change(screen.getByLabelText('₹ Limit'), { target: { value: '75000' } });
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(API.spendingLimits.update).toHaveBeenCalledWith(
        expect.objectContaining({ global_limit: 75000 })
      )
    );
  });

  it('confirms a successful save', async () => {
    await mount();
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Spending Limits Saved' }))
    );
  });

  it.each([
    ['a blank limit', ''],
    ['non-numeric text', 'abc'],
    ['a negative limit', '-100'],
  ])('refuses to save %s', async (_label, value) => {
    await mount();
    fireEvent.change(screen.getByLabelText('₹ Limit'), { target: { value } });
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Invalid limit' }))
    );
    expect(API.spendingLimits.update).not.toHaveBeenCalled();
  });

  it('accepts zero as a deliberate limit', async () => {
    await mount();
    fireEvent.change(screen.getByLabelText('₹ Limit'), { target: { value: '0' } });
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() => expect(API.spendingLimits.update).toHaveBeenCalled());
  });

  it('rejects an implausibly large limit', async () => {
    await mount();
    fireEvent.change(screen.getByLabelText('₹ Limit'), { target: { value: '1000000000000' } });
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(expect.objectContaining({ title: 'Too large' }))
    );
    expect(API.spendingLimits.update).not.toHaveBeenCalled();
  });

  it('reports a save failure with the backend message', async () => {
    asMock(API.spendingLimits.update).mockRejectedValue(new Error('disk full'));
    await mount();
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(
        expect.objectContaining({ title: 'Save Failed', description: 'disk full' })
      )
    );
  });

  it('falls back to generic copy for a non-Error rejection', async () => {
    asMock(API.spendingLimits.update).mockRejectedValue('kaboom');
    await mount();
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(
        expect.objectContaining({ description: 'Could not save spending limits.' })
      )
    );
  });

  it('disables the button while saving', async () => {
    let release: () => void;
    asMock(API.spendingLimits.update).mockReturnValue(new Promise<void>((r) => (release = r)));
    await mount();
    fireEvent.click(screen.getByRole('button', { name: /save changes/i }));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /saving/i })).toHaveProperty('disabled', true)
    );
    release!();
  });
});
