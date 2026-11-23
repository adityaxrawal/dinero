// M16 follow-up: restoring a licence after a reinstall.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import RestoreLicenseForm from '@/pages/settings/RestoreLicenseForm';

const restore = vi.fn();
vi.mock('@/lib/ipc', () => ({
  API: { licensing: { restore: (...a: unknown[]) => restore(...a) } },
}));

beforeEach(() => vi.clearAllMocks());

describe('RestoreLicenseForm', () => {
  it('restores with trimmed email and payment id, then reloads', async () => {
    restore.mockResolvedValue({ status: 'restored' });
    const reload = vi.fn().mockResolvedValue(undefined);
    render(<RestoreLicenseForm reload={reload} setError={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: /Restore licence/ }));
    fireEvent.change(screen.getByLabelText('Restore email'), { target: { value: ' u@e.com ' } });
    fireEvent.change(screen.getByLabelText('Restore payment ID'), { target: { value: ' pay_1 ' } });
    fireEvent.click(screen.getByRole('button', { name: /^Restore licence$/ }));
    await waitFor(() => expect(reload).toHaveBeenCalled());
    expect(restore).toHaveBeenCalledWith('u@e.com', 'pay_1');
  });

  it('shows the server error and keeps the form open', async () => {
    restore.mockRejectedValue({ message: 'No licence matches that email and payment id.' });
    const setError = vi.fn();
    render(<RestoreLicenseForm reload={vi.fn()} setError={setError} />);
    fireEvent.click(screen.getByRole('button', { name: /Restore licence/ }));
    fireEvent.change(screen.getByLabelText('Restore email'), { target: { value: 'u@e.com' } });
    fireEvent.change(screen.getByLabelText('Restore payment ID'), { target: { value: 'pay_x' } });
    fireEvent.click(screen.getByRole('button', { name: /^Restore licence$/ }));
    await waitFor(() =>
      expect(setError).toHaveBeenLastCalledWith(expect.stringMatching(/No licence/))
    );
    expect(screen.getByLabelText('Restore email')).toBeInTheDocument();
  });
});
