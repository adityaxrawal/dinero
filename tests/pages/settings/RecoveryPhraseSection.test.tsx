// The recovery phrase bypasses this Mac's hardware-bound key protection --
// anyone holding the 24 words can decrypt the user's financial data on any
// computer. The guard that matters is that it is never fetched, and never
// rendered, without an explicit confirmation.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import RecoveryPhraseSection from '@/pages/settings/RecoveryPhraseSection';
import { API } from '@/lib/ipc';
import { confirmAction } from '@/lib/confirmDialog';

vi.mock('@/lib/ipc', () => ({ API: { auth: { getRecoveryPhrase: vi.fn() } } }));
vi.mock('@/lib/confirmDialog', () => ({ confirmAction: vi.fn() }));

const asMock = (fn: unknown) => fn as ReturnType<typeof vi.fn>;
const PHRASE = Array.from({ length: 24 }, (_, i) => `word${i + 1}`).join(' ');

const viewButton = () => screen.getByRole('button', { name: /View Recovery Phrase|Generating/ });

beforeEach(() => {
  vi.clearAllMocks();
  asMock(confirmAction).mockResolvedValue(true);
  asMock(API.auth.getRecoveryPhrase).mockResolvedValue(PHRASE);
  vi.spyOn(window, 'alert').mockImplementation(() => {});
});

afterEach(() => vi.restoreAllMocks());

describe('RecoveryPhraseSection', () => {
  it('shows no phrase until it is explicitly requested', () => {
    render(<RecoveryPhraseSection />);

    expect(screen.queryByText(PHRASE)).not.toBeInTheDocument();
    expect(viewButton()).toBeInTheDocument();
  });

  it('never reaches the backend when the confirmation is declined', async () => {
    asMock(confirmAction).mockResolvedValue(false);
    render(<RecoveryPhraseSection />);

    fireEvent.click(viewButton());

    await waitFor(() => expect(confirmAction).toHaveBeenCalledTimes(1));
    expect(API.auth.getRecoveryPhrase).not.toHaveBeenCalled();
    expect(screen.queryByText(PHRASE)).not.toBeInTheDocument();
  });

  it('warns about the hardware-protection bypass before revealing anything', async () => {
    render(<RecoveryPhraseSection />);
    fireEvent.click(viewButton());

    await waitFor(() => expect(confirmAction).toHaveBeenCalled());
    expect(asMock(confirmAction).mock.calls[0][0]).toContain(
      "bypasses this Mac's hardware-bound protection"
    );
  });

  it('reveals the phrase once confirmed and replaces the trigger', async () => {
    render(<RecoveryPhraseSection />);
    fireEvent.click(viewButton());

    await waitFor(() => expect(screen.getByText(PHRASE)).toBeInTheDocument());
    // The button is gone, so the phrase cannot be re-fetched by a stray click.
    expect(
      screen.queryByRole('button', { name: /View Recovery Phrase/ })
    ).not.toBeInTheDocument();
  });

  it('surfaces a retrieval failure and stays re-armed', async () => {
    asMock(API.auth.getRecoveryPhrase).mockRejectedValue(new Error('keychain locked'));
    render(<RecoveryPhraseSection />);

    fireEvent.click(viewButton());

    await waitFor(() =>
      expect(window.alert).toHaveBeenCalledWith(
        'Failed to retrieve recovery phrase: keychain locked'
      )
    );
    expect(screen.queryByText(PHRASE)).not.toBeInTheDocument();
    await waitFor(() => expect(viewButton()).toBeEnabled());
  });
});
