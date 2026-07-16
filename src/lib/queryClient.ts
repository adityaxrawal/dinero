import { QueryClient } from '@tanstack/react-query';

/**
 * TASK-FE-003 (Doc 30): ~30s staleTime — background Tokio workers (Gmail
 * poll, historical scan, reconciliation) mutate the database independently
 * of user action, so cached results go stale faster than in a typical
 * user-driven app. Event-driven invalidation (`useIpcQueryInvalidation`) is
 * the primary freshness mechanism; this is just the fallback floor for
 * anything not covered by a specific event.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});
