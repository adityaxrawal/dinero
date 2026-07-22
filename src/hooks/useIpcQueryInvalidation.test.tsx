import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useIpcQueryInvalidation } from './useIpcQueryInvalidation';
import { queryKeys } from '@/lib/queryKeys';

describe('useIpcQueryInvalidation', () => {
  it('is a no-op outside the Tauri runtime (no __TAURI_INTERNALS__ in jsdom) and does not throw', () => {
    const queryClient = new QueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    expect(() => {
      const { unmount } = renderHook(() => useIpcQueryInvalidation(), { wrapper });
      unmount();
    }).not.toThrow();
  });
});

describe('useIpcQueryInvalidation inside the Tauri runtime', () => {
  const listenHandlers: Record<string, (event: unknown) => void> = {};

  beforeEach(() => {
    (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    for (const k of Object.keys(listenHandlers)) delete listenHandlers[k];
    vi.doMock('@tauri-apps/api/event', () => ({
      listen: vi.fn((event: string, handler: (e: unknown) => void) => {
        listenHandlers[event] = handler;
        return Promise.resolve(() => {
          delete listenHandlers[event];
        });
      }),
    }));
  });

  afterEach(() => {
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.doUnmock('@tauri-apps/api/event');
    vi.resetModules();
  });

  it('test_transaction_created_invalidates_correct_query_keys', async () => {
    const { useIpcQueryInvalidation: freshHook } = await import('./useIpcQueryInvalidation');
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    renderHook(() => freshHook(), { wrapper });

    await waitFor(() => expect(listenHandlers['transaction_created']).toBeDefined());
    listenHandlers['transaction_created']({ payload: { observation_id: 'obs_1' } });

    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.transactions.all() });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.dashboard.all() });
  });
});
