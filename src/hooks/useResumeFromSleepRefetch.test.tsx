import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useResumeFromSleepRefetch, SLEEP_GAP_THRESHOLD_MS } from './useResumeFromSleepRefetch';
import { queryKeys } from '@/lib/queryKeys';

function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true });
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('useResumeFromSleepRefetch', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
  });

  function renderWithClient() {
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    renderHook(() => useResumeFromSleepRefetch(), { wrapper });
    return invalidateSpy;
  }

  it('test_resume_from_sleep_triggers_full_refetch: invalidates transactions and dashboard after a long hidden gap', () => {
    const invalidateSpy = renderWithClient();

    setVisibility('hidden');
    vi.advanceTimersByTime(SLEEP_GAP_THRESHOLD_MS + 5000);
    setVisibility('visible');

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.transactions.all() });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.dashboard.all() });
  });

  it('does not refetch for a brief hidden gap (ordinary alt-tab, not a real sleep/background)', () => {
    const invalidateSpy = renderWithClient();

    setVisibility('hidden');
    vi.advanceTimersByTime(5000);
    setVisibility('visible');

    expect(invalidateSpy).not.toHaveBeenCalled();
  });

  it('does nothing on a spurious visible event with no prior hidden transition', () => {
    const invalidateSpy = renderWithClient();
    setVisibility('visible');
    expect(invalidateSpy).not.toHaveBeenCalled();
  });
});
