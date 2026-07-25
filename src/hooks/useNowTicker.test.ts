import { describe, it, expect, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useNowTicker } from './useNowTicker';

describe('useNowTicker', () => {
  it('ticks every second while active', () => {
    vi.useFakeTimers();
    const { result } = renderHook(({ active }) => useNowTicker(active), {
      initialProps: { active: true },
    });
    const first = result.current;

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(result.current).toBeGreaterThan(first);

    vi.useRealTimers();
  });

  it('does not tick while inactive', () => {
    vi.useFakeTimers();
    const { result } = renderHook(({ active }) => useNowTicker(active), {
      initialProps: { active: false },
    });
    const first = result.current;

    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(result.current).toBe(first);

    vi.useRealTimers();
  });

  it('resumes ticking when active flips from false to true', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ active }) => useNowTicker(active), {
      initialProps: { active: false },
    });
    const first = result.current;

    rerender({ active: true });
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(result.current).toBeGreaterThan(first);

    vi.useRealTimers();
  });
});
