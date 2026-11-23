import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PortableBackupSettings from './PortableBackupSettings';

const dialog = vi.hoisted(() => ({ save: vi.fn(), open: vi.fn(), ask: vi.fn() }));
const api = vi.hoisted(() => ({ exportBackup: vi.fn(), restorePortableBackup: vi.fn() }));

vi.mock('@tauri-apps/plugin-dialog', () => dialog);
vi.mock('@/lib/ipc', () => ({ API: { db: api } }));

const fill = (password: string, confirm: string) => {
  fireEvent.change(screen.getByLabelText('Backup password'), { target: { value: password } });
  fireEvent.change(screen.getByLabelText(/Confirm password/), { target: { value: confirm } });
};

describe('PortableBackupSettings', () => {
  beforeEach(() => vi.clearAllMocks());

  it('disables export until the password is long enough and confirmed', () => {
    render(<PortableBackupSettings />);
    const exportButton = screen.getByRole('button', { name: /Export backup/ });
    fill('short', 'short');
    expect(exportButton).toBeDisabled();
    fill('long enough', 'different');
    expect(exportButton).toBeDisabled();
    fill('long enough', 'long enough');
    expect(exportButton).toBeEnabled();
  });

  it('exports to the chosen path with the password', async () => {
    dialog.save.mockResolvedValue('/tmp/x.dinero');
    api.exportBackup.mockResolvedValue('/tmp/x.dinero');
    render(<PortableBackupSettings />);
    fill('long enough', 'long enough');
    fireEvent.click(screen.getByRole('button', { name: /Export backup/ }));
    await waitFor(() =>
      expect(api.exportBackup).toHaveBeenCalledWith('/tmp/x.dinero', 'long enough')
    );
  });

  it('restores only after confirmation', async () => {
    dialog.open.mockResolvedValue('/tmp/x.dinero');
    dialog.ask.mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    api.restorePortableBackup.mockResolvedValue('ok');
    render(<PortableBackupSettings />);
    fill('long enough', '');
    const restore = screen.getByRole('button', { name: /Restore from file/ });
    fireEvent.click(restore);
    await waitFor(() => expect(dialog.ask).toHaveBeenCalledTimes(1));
    expect(api.restorePortableBackup).not.toHaveBeenCalled();
    fireEvent.click(restore);
    await waitFor(() =>
      expect(api.restorePortableBackup).toHaveBeenCalledWith('/tmp/x.dinero', 'long enough')
    );
  });
});
