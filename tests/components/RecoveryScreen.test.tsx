// Review H11: the startup recovery screen restores from the phrase and
// relaunches, or resets app data after confirmation.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import RecoveryScreen from '@/components/RecoveryScreen';

const restoreFromPhrase = vi.fn();
const restart = vi.fn();
const resetAppData = vi.fn();
vi.mock('@/lib/ipc', () => ({
  API: {
    recovery: {
      restoreFromPhrase: (...a: unknown[]) => restoreFromPhrase(...a),
      restart: () => restart(),
      resetAppData: () => resetAppData(),
    },
  },
}));

const PHRASE = 'one two three four five six seven eight nine ten eleven twelve';

beforeEach(() => vi.clearAllMocks());

describe('RecoveryScreen', () => {
  it('restores with the trimmed phrase, then restarts', async () => {
    restoreFromPhrase.mockResolvedValue('ok');
    render(<RecoveryScreen />);
    const button = screen.getByRole('button', { name: /Restore and restart/ });
    expect(button).toBeDisabled();
    fireEvent.change(screen.getByLabelText('Recovery phrase'), {
      target: { value: ` ${PHRASE} ` },
    });
    fireEvent.click(button);
    await waitFor(() => expect(restart).toHaveBeenCalled());
    expect(restoreFromPhrase).toHaveBeenCalledWith(PHRASE);
  });

  it('shows the error and does not restart when the phrase is wrong', async () => {
    restoreFromPhrase.mockRejectedValue({
      message: 'Recovery phrase did not decrypt the database',
    });
    render(<RecoveryScreen />);
    fireEvent.change(screen.getByLabelText('Recovery phrase'), { target: { value: PHRASE } });
    fireEvent.click(screen.getByRole('button', { name: /Restore and restart/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/did not decrypt/);
    expect(restart).not.toHaveBeenCalled();
  });

  it('resets app data only after confirmation', () => {
    const confirm = vi
      .spyOn(window, 'confirm')
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true);
    render(<RecoveryScreen />);
    const reset = screen.getByRole('button', { name: /Reset app data/ });
    fireEvent.click(reset);
    expect(resetAppData).not.toHaveBeenCalled();
    fireEvent.click(reset);
    expect(resetAppData).toHaveBeenCalled();
    confirm.mockRestore();
  });
});
