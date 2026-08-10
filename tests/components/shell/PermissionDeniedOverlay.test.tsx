// TASK-DESK-004: the two OS-permission denials differ by severity, and the
// difference is the point. Keychain holds both the Gmail tokens and the
// SQLite encryption key, so its denial is a non-dismissable hard fail;
// notifications are cosmetic, so that one must never block the app.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react';
import PermissionDeniedOverlay from '@/components/shell/PermissionDeniedOverlay';

interface SystemWarningPayload {
  warning_type: string;
  message: string;
  severity: 'hard_fail' | 'soft_fail';
}

let emit: (payload: SystemWarningPayload) => void = () => {};
vi.mock('@/hooks/useIpcListen', () => ({
  useIpcListen: (_event: string, handler: (p: SystemWarningPayload) => void) => {
    emit = handler;
  },
}));

const openUrl = vi.fn();
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: (...a: unknown[]) => openUrl(...a) }));

const keychainWarning: SystemWarningPayload = {
  warning_type: 'keychain_denied',
  message: 'Dinero could not read its encryption key from Keychain.',
  severity: 'hard_fail',
};
const notificationWarning: SystemWarningPayload = {
  warning_type: 'notification_denied',
  message: 'Notifications are turned off for Dinero.',
  severity: 'soft_fail',
};

const overlay = () => screen.queryByTestId('permission-denied-overlay');
const note = () => screen.queryByTestId('notification-permission-note');

const send = (payload: SystemWarningPayload) => act(() => emit(payload));

beforeEach(() => {
  vi.clearAllMocks();
  openUrl.mockResolvedValue(undefined);
});

afterEach(() => vi.restoreAllMocks());

describe('PermissionDeniedOverlay', () => {
  it('shows nothing until a warning arrives', () => {
    render(<PermissionDeniedOverlay />);

    expect(overlay()).not.toBeInTheDocument();
    expect(note()).not.toBeInTheDocument();
  });

  it('raises a blocking alertdialog on keychain denial', () => {
    render(<PermissionDeniedOverlay />);
    send(keychainWarning);

    expect(overlay()).toBeInTheDocument();
    expect(screen.getByRole('alertdialog')).toHaveAttribute('aria-modal', 'true');
    expect(screen.getByText(keychainWarning.message)).toBeInTheDocument();
  });

  it('offers no way to dismiss the keychain overlay', () => {
    // There is no safe degraded mode -- both the Gmail tokens and the DB key
    // are unreachable, so this must not be closeable.
    render(<PermissionDeniedOverlay />);
    send(keychainWarning);

    expect(screen.queryByRole('button', { name: /dismiss/i })).not.toBeInTheDocument();
    expect(screen.getAllByRole('button')).toHaveLength(1);
  });

  it('deep-links to the Privacy pane for keychain', async () => {
    render(<PermissionDeniedOverlay />);
    send(keychainWarning);

    fireEvent.click(
      screen.getByRole('button', { name: 'Open System Settings to grant Keychain access' })
    );

    await waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith(
        'x-apple.systempreferences:com.apple.preference.security?Privacy'
      )
    );
  });

  it('shows notification denial as a non-blocking, dismissable note', () => {
    render(<PermissionDeniedOverlay />);
    send(notificationWarning);

    expect(note()).toBeInTheDocument();
    expect(overlay()).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss notification permission note' }));
    expect(note()).not.toBeInTheDocument();
  });

  it('re-shows a dismissed note when the warning fires again', () => {
    render(<PermissionDeniedOverlay />);
    send(notificationWarning);
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss notification permission note' }));
    expect(note()).not.toBeInTheDocument();

    send(notificationWarning);
    expect(note()).toBeInTheDocument();
  });

  it('deep-links to the Notifications pane', async () => {
    render(<PermissionDeniedOverlay />);
    send(notificationWarning);

    fireEvent.click(screen.getByRole('button', { name: 'Open Settings' }));

    await waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith(
        'x-apple.systempreferences:com.apple.preference.notifications'
      )
    );
  });

  it('keeps the blocking overlay when a soft failure arrives alongside it', () => {
    render(<PermissionDeniedOverlay />);
    send(keychainWarning);
    send(notificationWarning);

    expect(overlay()).toBeInTheDocument();
    expect(note()).toBeInTheDocument();
  });

  it('survives a failure to open System Settings', async () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    openUrl.mockRejectedValue(new Error('no opener'));
    render(<PermissionDeniedOverlay />);
    send(keychainWarning);

    fireEvent.click(
      screen.getByRole('button', { name: 'Open System Settings to grant Keychain access' })
    );

    await waitFor(() => expect(consoleError).toHaveBeenCalled());
    // Still up: the user's permission problem is not resolved.
    expect(overlay()).toBeInTheDocument();
  });
});
