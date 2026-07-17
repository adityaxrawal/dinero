import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useIpcInvoke } from './useIpcInvoke';

vi.mock('@/lib/ipc', () => ({
  invokeCommand: vi.fn(),
}));

const toastSpy = vi.fn();
vi.mock('@/hooks/use-toast', () => ({
  toast: (...args: unknown[]) => toastSpy(...args),
}));

import { invokeCommand } from '@/lib/ipc';

describe('useIpcInvoke', () => {
  beforeEach(() => {
    toastSpy.mockClear();
    vi.mocked(invokeCommand).mockReset();
  });

  it('resolves and exposes no error on success, without toasting', async () => {
    vi.mocked(invokeCommand).mockResolvedValueOnce({ ok: true });
    const { result } = renderHook(() => useIpcInvoke('some_command'));

    await act(async () => {
      const value = await result.current.invoke();
      expect(value).toEqual({ ok: true });
    });

    expect(result.current.error).toBeNull();
    expect(toastSpy).not.toHaveBeenCalled();
  });

  /**
   * TASK-FE-018 (Doc 30): "a global toast queue triggered automatically by
   * any failed useIpcInvoke call, so components don't need to manually
   * wire error toasts per mutation."
   */
  it('auto-toasts via errorMapping on failure, still exposes the error, and still rethrows', async () => {
    vi.mocked(invokeCommand).mockRejectedValueOnce({ code: 'NETWORK_ERROR', message: 'ECONNREFUSED' });
    const { result } = renderHook(() => useIpcInvoke('some_command'));

    await act(async () => {
      await expect(result.current.invoke()).rejects.toEqual({ code: 'NETWORK_ERROR', message: 'ECONNREFUSED' });
    });

    expect(result.current.error).toEqual({ code: 'NETWORK_ERROR', message: 'ECONNREFUSED' });
    expect(toastSpy).toHaveBeenCalledTimes(1);
    expect(toastSpy.mock.calls[0][0]).toMatchObject({
      variant: 'destructive',
      description: expect.stringMatching(/internet connection/i),
    });
  });
});
