import { describe, it, expect } from 'vitest';
import { renderHook } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { useIpcQueryInvalidation } from './useIpcQueryInvalidation';

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
