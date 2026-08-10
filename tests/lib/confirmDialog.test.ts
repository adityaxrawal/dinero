import { describe, it, expect, vi, beforeEach } from 'vitest';
import { confirmAction, confirmDeleteTransaction } from '@/lib/confirmDialog';

const ask = vi.fn();
vi.mock('@tauri-apps/plugin-dialog', () => ({ ask: (...a: unknown[]) => ask(...a) }));

beforeEach(() => {
  vi.clearAllMocks();
  ask.mockResolvedValue(true);
});

describe('confirmAction', () => {
  it('asks through the native dialog with a warning kind', async () => {
    await confirmAction('Really?', 'Confirm');
    expect(ask).toHaveBeenCalledWith('Really?', { title: 'Confirm', kind: 'warning' });
  });

  it.each([true, false])('returns the native answer (%s)', async (answer) => {
    ask.mockResolvedValue(answer);
    expect(await confirmAction('Really?', 'Confirm')).toBe(answer);
  });

  it('falls back to window.confirm outside the Tauri shell', async () => {
    ask.mockRejectedValue(new Error('plugin unavailable'));
    const spy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    expect(await confirmAction('Really?', 'Confirm')).toBe(true);
    expect(spy).toHaveBeenCalledWith('Really?');
    spy.mockRestore();
  });

  it('honours a declined fallback confirm', async () => {
    ask.mockRejectedValue(new Error('plugin unavailable'));
    const spy = vi.spyOn(window, 'confirm').mockReturnValue(false);
    expect(await confirmAction('Really?', 'Confirm')).toBe(false);
    spy.mockRestore();
  });
});

describe('confirmDeleteTransaction', () => {
  it('warns that the delete cannot be undone', async () => {
    await confirmDeleteTransaction();
    expect(ask).toHaveBeenCalledWith(
      expect.stringContaining('cannot be undone'),
      expect.objectContaining({ title: 'Delete Transaction' })
    );
  });
});
