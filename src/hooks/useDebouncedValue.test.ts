import { describe, it, expect, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useDebouncedValue } from './useDebouncedValue';

describe('useDebouncedValue', () => {
  it('does not update immediately on change', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value, 300), {
      initialProps: { value: 'a' },
    });
    expect(result.current).toBe('a');

    rerender({ value: 'ab' });
    expect(result.current).toBe('a');

    act(() => {
      vi.advanceTimersByTime(299);
    });
    expect(result.current).toBe('a');

    vi.useRealTimers();
  });

  it('updates after the delay elapses', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value, 300), {
      initialProps: { value: 'a' },
    });

    rerender({ value: 'ab' });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(result.current).toBe('ab');

    vi.useRealTimers();
  });

  it('resets the timer on rapid successive changes (only the final value wins)', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ value }) => useDebouncedValue(value, 300), {
      initialProps: { value: 'a' },
    });

    rerender({ value: 'ab' });
    act(() => {
      vi.advanceTimersByTime(200);
    });
    rerender({ value: 'abc' });
    act(() => {
      vi.advanceTimersByTime(200);
    });
    // Only 200ms since the last change — should not have updated yet.
    expect(result.current).toBe('a');

    act(() => {
      vi.advanceTimersByTime(100);
    });
    expect(result.current).toBe('abc');

    vi.useRealTimers();
  });
});
