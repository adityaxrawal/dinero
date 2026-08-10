// The two-step destructive wipe (TASK-FE-014): warning -> type-to-confirm
// "DELETE MY DATA" -> `settings_delete_account`. This flow deletes every
// transaction, statement, instrument, OAuth token and Keychain key on the
// device and cannot be undone, so the guards that stand between a stray
// click and that call are what these tests pin down.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import DeleteAccountSection from '@/components/settings/DeleteAccountSection';
import { API } from '@/lib/ipc';

vi.mock('@/lib/ipc', () => ({
  API: { dev: { resetDatabase: vi.fn() } },
}));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const reload = vi.fn();

const deleteButton = () => screen.getByRole('button', { name: 'Permanently delete my data' });
const confirmInput = () => screen.getByLabelText('Confirmation phrase');

/** Open the modal and advance past the step-1 warning. */
function openToConfirmStep() {
  render(<DeleteAccountSection />);
  fireEvent.click(screen.getByRole('button', { name: /Delete My Data/i }));
  fireEvent.click(screen.getByRole('button', { name: 'I understand, continue to confirmation' }));
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('location', { ...window.location, reload });
  vi.spyOn(window, 'alert').mockImplementation(() => {});
  asMock(API.dev.resetDatabase).mockResolvedValue('ok');
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('DeleteAccountSection', () => {
  it('does not open the modal until the trigger is clicked', () => {
    render(<DeleteAccountSection />);
    expect(screen.queryByText('This cannot be undone.')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Delete My Data/i }));
    expect(screen.getByText('This cannot be undone.')).toBeInTheDocument();
  });

  it('requires the step-1 warning before the confirmation phrase is reachable', () => {
    render(<DeleteAccountSection />);
    fireEvent.click(screen.getByRole('button', { name: /Delete My Data/i }));

    // Step 1 has no text input -- the phrase cannot be typed yet.
    expect(screen.queryByLabelText('Confirmation phrase')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'I understand, continue to confirmation' }));
    expect(confirmInput()).toBeInTheDocument();
  });

  it('keeps the delete button disabled until the phrase matches exactly', () => {
    openToConfirmStep();
    expect(deleteButton()).toBeDisabled();

    fireEvent.change(confirmInput(), { target: { value: 'delete my data' } });
    expect(deleteButton()).toBeDisabled();

    fireEvent.change(confirmInput(), { target: { value: 'DELETE MY DATA ' } });
    expect(deleteButton()).toBeDisabled();

    fireEvent.change(confirmInput(), { target: { value: 'DELETE MY DATA' } });
    expect(deleteButton()).toBeEnabled();
  });

  it('never calls the backend for a near-miss phrase', () => {
    openToConfirmStep();
    fireEvent.change(confirmInput(), { target: { value: 'Delete My Data' } });
    fireEvent.click(deleteButton());

    expect(API.dev.resetDatabase).not.toHaveBeenCalled();
  });

  it('wipes and reloads once the exact phrase is confirmed', async () => {
    openToConfirmStep();
    fireEvent.change(confirmInput(), { target: { value: 'DELETE MY DATA' } });
    fireEvent.click(deleteButton());

    await waitFor(() => expect(API.dev.resetDatabase).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(reload).toHaveBeenCalledTimes(1));
  });

  it('clears the typed phrase and returns to step 1 when cancelled and reopened', () => {
    openToConfirmStep();
    fireEvent.change(confirmInput(), { target: { value: 'DELETE MY DATA' } });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel data deletion' }));

    fireEvent.click(screen.getByRole('button', { name: /Delete My Data/i }));
    // Back at the step-1 warning, not the pre-armed confirm step.
    expect(screen.getByText('This cannot be undone.')).toBeInTheDocument();
    expect(screen.queryByLabelText('Confirmation phrase')).not.toBeInTheDocument();
  });

  it('explains the manual restart instead of reloading in Tauri dev mode', async () => {
    asMock(API.dev.resetDatabase).mockRejectedValue('DEV_RESTART_REQUIRED');
    openToConfirmStep();
    fireEvent.change(confirmInput(), { target: { value: 'DELETE MY DATA' } });
    fireEvent.click(deleteButton());

    await waitFor(() =>
      expect(window.alert).toHaveBeenCalledWith(expect.stringContaining('Development Mode'))
    );
    // The wipe succeeded; reloading would break the Vite dev server, so the
    // button intentionally stays in its in-flight state.
    expect(reload).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Permanently delete my data' })).toHaveTextContent(
      'Deleting…'
    );
  });

  it('re-arms the delete button after a genuine failure', async () => {
    asMock(API.dev.resetDatabase).mockRejectedValue(new Error('disk busy'));
    vi.spyOn(console, 'error').mockImplementation(() => {});
    openToConfirmStep();
    fireEvent.change(confirmInput(), { target: { value: 'DELETE MY DATA' } });
    fireEvent.click(deleteButton());

    await waitFor(() => expect(window.alert).toHaveBeenCalledWith('Failed to reset database'));
    expect(reload).not.toHaveBeenCalled();
    // Retryable, not wedged on "Deleting…".
    await waitFor(() => expect(deleteButton()).toBeEnabled());
    expect(deleteButton()).toHaveTextContent('Permanently Delete');
  });
});
