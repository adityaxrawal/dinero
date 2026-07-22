// Doc 30 TASK-RT-003 acceptance: test_toast_auto_dismisses_after_timeout,
// test_toast_queue_caps_at_max_visible. use-toast.ts's module-level
// singleton store requires vi.resetModules() + a fresh dynamic import per
// test for isolation -- otherwise toasts queued by one test leak into the
// next via the shared `memoryState`.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

describe('use-toast', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetModules();
  });

  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it('test_toast_auto_dismisses_after_timeout', async () => {
    const { toast, useToast } = await import('./use-toast');
    const { renderHook, act } = await import('@testing-library/react');
    const { result } = renderHook(() => useToast());

    act(() => {
      toast({ title: 'Hello' });
    });
    expect(result.current.toasts).toHaveLength(1);
    expect(result.current.toasts[0].open).toBe(true);

    act(() => {
      vi.advanceTimersByTime(6000);
    });
    expect(result.current.toasts[0].open).toBe(false);
  });

  it('does not auto-dismiss before the timeout elapses', async () => {
    const { toast, useToast } = await import('./use-toast');
    const { renderHook, act } = await import('@testing-library/react');
    const { result } = renderHook(() => useToast());

    act(() => {
      toast({ title: 'Hello' });
    });
    act(() => {
      vi.advanceTimersByTime(4000);
    });
    expect(result.current.toasts[0].open).toBe(true);
  });

  it('test_toast_queue_caps_at_max_visible', async () => {
    const { toast, useToast } = await import('./use-toast');
    const { renderHook, act } = await import('@testing-library/react');
    const { result } = renderHook(() => useToast());

    act(() => {
      toast({ title: 'One' });
      toast({ title: 'Two' });
      toast({ title: 'Three' });
      toast({ title: 'Four' });
    });

    expect(result.current.toasts).toHaveLength(3);
    // Most recent 3 are kept -- "Four" pushed "One" out.
    expect(result.current.toasts.map((t) => t.title)).toEqual(['Four', 'Three', 'Two']);
  });
});
