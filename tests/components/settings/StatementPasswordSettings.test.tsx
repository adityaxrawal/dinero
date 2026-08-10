// TASK-FE-015: stored statement passwords are listed by instrument identity
// and never rendered. Deleting one is confirmed through the native Tauri
// dialog, falling back to window.confirm when the plugin is unavailable
// (e.g. a browser dev server rather than the packaged app).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import StatementPasswordSettings from '@/components/settings/StatementPasswordSettings';
import { API } from '@/lib/ipc';

const ask = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({ ask: (...args: unknown[]) => ask(...args) }));
vi.mock('@/lib/ipc', () => ({
  API: { pdfPasswords: { list: vi.fn(), delete: vi.fn() } },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;

const pw = (over = {}) => ({
  id: 'pw1',
  issuer_name: 'HDFC Bank',
  masked_identifier: '4321',
  success_count: 3,
  last_used_at: '2026-05-02T10:00:00Z',
  ...over,
});

const forgetButton = () => screen.getByRole('button', { name: /Forget/ });

beforeEach(() => {
  vi.clearAllMocks();
  ask.mockResolvedValue(true);
  asMock(API.pdfPasswords.list).mockResolvedValue([pw()]);
  asMock(API.pdfPasswords.delete).mockResolvedValue(undefined);
  vi.spyOn(window, 'alert').mockImplementation(() => {});
});

afterEach(() => vi.restoreAllMocks());

describe('StatementPasswordSettings', () => {
  it('says so when nothing has been stored', async () => {
    asMock(API.pdfPasswords.list).mockResolvedValue([]);
    render(<StatementPasswordSettings />);

    await waitFor(() => expect(screen.getByText('No stored passwords yet.')).toBeInTheDocument());
  });

  it('identifies each entry by instrument without showing the password', async () => {
    render(<StatementPasswordSettings />);

    await waitFor(() => expect(screen.getByText('HDFC Bank')).toBeInTheDocument());
    expect(screen.getByText(/•••• 4321/)).toBeInTheDocument();
    expect(screen.getByText(/Used successfully 3 times/)).toBeInTheDocument();
  });

  it('singularises a single successful use and omits an absent last-used date', async () => {
    asMock(API.pdfPasswords.list).mockResolvedValue([
      pw({ success_count: 1, last_used_at: null }),
    ]);
    render(<StatementPasswordSettings />);

    await waitFor(() => expect(screen.getByText(/Used successfully 1 time/)).toBeInTheDocument());
    expect(screen.queryByText(/last on/)).not.toBeInTheDocument();
  });

  it('keeps the password when the confirmation is declined', async () => {
    ask.mockResolvedValue(false);
    render(<StatementPasswordSettings />);
    await waitFor(() => expect(screen.getByText('HDFC Bank')).toBeInTheDocument());

    fireEvent.click(forgetButton());

    await waitFor(() => expect(ask).toHaveBeenCalled());
    expect(API.pdfPasswords.delete).not.toHaveBeenCalled();
  });

  it('deletes and reloads the list once confirmed', async () => {
    render(<StatementPasswordSettings />);
    await waitFor(() => expect(screen.getByText('HDFC Bank')).toBeInTheDocument());
    asMock(API.pdfPasswords.list).mockResolvedValue([]);

    fireEvent.click(forgetButton());

    await waitFor(() => expect(API.pdfPasswords.delete).toHaveBeenCalledWith('pw1'));
    await waitFor(() => expect(screen.getByText('No stored passwords yet.')).toBeInTheDocument());
  });

  it('falls back to window.confirm when the native dialog is unavailable', async () => {
    ask.mockRejectedValue(new Error('plugin missing'));
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    render(<StatementPasswordSettings />);
    await waitFor(() => expect(screen.getByText('HDFC Bank')).toBeInTheDocument());

    fireEvent.click(forgetButton());

    await waitFor(() => expect(confirmSpy).toHaveBeenCalled());
    await waitFor(() => expect(API.pdfPasswords.delete).toHaveBeenCalledWith('pw1'));
  });

  it('reports a failed delete without dropping the row', async () => {
    asMock(API.pdfPasswords.delete).mockRejectedValue(new Error('db locked'));
    render(<StatementPasswordSettings />);
    await waitFor(() => expect(screen.getByText('HDFC Bank')).toBeInTheDocument());

    fireEvent.click(forgetButton());

    await waitFor(() =>
      expect(window.alert).toHaveBeenCalledWith(expect.stringContaining('Failed to delete password'))
    );
    expect(screen.getByText('HDFC Bank')).toBeInTheDocument();
    await waitFor(() => expect(forgetButton()).toBeEnabled());
  });
});
