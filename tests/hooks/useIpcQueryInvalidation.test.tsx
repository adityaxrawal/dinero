import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useIpcQueryInvalidation } from '@/hooks/useIpcQueryInvalidation';
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
    const { useIpcQueryInvalidation: freshHook } = await import('@/hooks/useIpcQueryInvalidation');
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries');
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    renderHook(() => freshHook(), { wrapper });

    await waitFor(() => expect(listenHandlers['transaction_created']).toBeDefined());
    listenHandlers['transaction_created']({ payload: { observation_id: 'obs_1' } });

    // Invalidations are batched (audit B-2), so they land within the window.
    await waitFor(
      () => {
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.transactions.all() });
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.dashboard.all() });
        // Audit N-B1: a new alert can create a new card.
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: queryKeys.instruments.all() });
      },
      { timeout: 2000 }
    );
  });
});

describe('createInvalidationBatcher (audit B-2)', () => {
  afterEach(() => vi.useRealTimers());

  it('collapses a burst of events into one invalidation per key', async () => {
    vi.useFakeTimers();
    const { createInvalidationBatcher, INVALIDATION_BATCH_MS } = await import(
      '@/hooks/useIpcQueryInvalidation'
    );
    const invalidate = vi.fn();
    const batcher = createInvalidationBatcher(invalidate);
    for (let i = 0; i < 50; i++) {
      batcher.add([queryKeys.transactions.all(), queryKeys.dashboard.all()]);
    }
    expect(invalidate).not.toHaveBeenCalled();
    vi.advanceTimersByTime(INVALIDATION_BATCH_MS);
    expect(invalidate).toHaveBeenCalledTimes(2);

    batcher.add([queryKeys.transactions.all()]);
    batcher.cancel();
    vi.advanceTimersByTime(INVALIDATION_BATCH_MS);
    expect(invalidate).toHaveBeenCalledTimes(2);
  });
});

describe('EVENT_INVALIDATIONS (audit F14-1, F12-3, F8-4)', () => {
  it('maps review, edit and statement events to every screen they change', async () => {
    const { EVENT_INVALIDATIONS } = await import('@/hooks/useIpcQueryInvalidation');
    const keysFor = (event: string) =>
      EVENT_INVALIDATIONS.find((e) => e.event === event)?.keys.map((k) => k[0]) ?? [];
    expect(keysFor('unassigned_changed')).toEqual(['reconciliation', 'dashboard']);
    expect(keysFor('transaction_updated')).toContain('instruments');
    expect(keysFor('scan_completed')).toContain('reconciliation');
    expect(keysFor('statement_parsed')).toEqual(['statements']);
    expect(keysFor('statement_instrument_confirmation_required')).toEqual(['statements']);
  });
});
