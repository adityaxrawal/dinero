// The recovery screen shown when the SQLite integrity check fails. Both of
// its actions are irreversible-ish (one restores over the live DB, the other
// deletes everything), so the in-flight lockout is the behaviour that matters.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import CorruptedDbScreen from '@/components/layout/appShell/CorruptedDbScreen';

const onRestore = vi.fn();
const onStartFresh = vi.fn();

const restoreBtn = () => screen.getByRole('button', { name: 'Restore database from backup' });
const freshBtn = () =>
  screen.getByRole('button', { name: 'Delete all local data and start fresh' });

function renderScreen(over: Record<string, unknown> = {}) {
  return render(
    <CorruptedDbScreen
      isRestoring={false}
      isStartingFresh={false}
      onRestore={onRestore}
      onStartFresh={onStartFresh}
      {...over}
    />
  );
}

beforeEach(() => vi.clearAllMocks());

describe('CorruptedDbScreen', () => {
  it('announces itself as a modal dialog with both recovery paths', () => {
    renderScreen();

    expect(screen.getByRole('dialog')).toHaveAttribute('aria-modal', 'true');
    expect(screen.getByText('Database Corrupted')).toBeInTheDocument();
    expect(restoreBtn()).toBeEnabled();
    expect(freshBtn()).toBeEnabled();
  });

  it('routes each button to its own handler', () => {
    renderScreen();

    fireEvent.click(restoreBtn());
    expect(onRestore).toHaveBeenCalledTimes(1);
    expect(onStartFresh).not.toHaveBeenCalled();

    fireEvent.click(freshBtn());
    expect(onStartFresh).toHaveBeenCalledTimes(1);
  });

  it('locks out both actions while a restore is in flight', () => {
    // Not just the button that was pressed: starting fresh mid-restore would
    // race two writers on the same database file.
    renderScreen({ isRestoring: true });

    expect(restoreBtn()).toBeDisabled();
    expect(freshBtn()).toBeDisabled();
    expect(restoreBtn()).toHaveTextContent('Restoring…');
  });

  it('locks out both actions while starting fresh', () => {
    renderScreen({ isStartingFresh: true });

    expect(restoreBtn()).toBeDisabled();
    expect(freshBtn()).toBeDisabled();
    expect(freshBtn()).toHaveTextContent('Starting Fresh…');
  });
});
