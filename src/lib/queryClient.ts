/**
 * The single React Query client instance shared by the whole application.
 *
 * Defaults here are tuned for a local desktop app rather than a web client:
 * data comes from a Rust backend over IPC on the same machine, so there is no
 * network latency to hide and no server that might have been updated by another
 * user. Freshness instead arrives through explicit cache invalidation driven by
 * backend events, which is why the automatic refetch behaviour is dialled down.
 */
import { QueryClient } from '@tanstack/react-query';

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Half a minute before a cached result is considered stale, which absorbs
      // rapid navigation between screens without re-querying on every mount.
      staleTime: 30_000,

      // Refocusing the desktop window is not a signal that local data changed;
      // event-driven invalidation covers that instead.
      refetchOnWindowFocus: false,

      // One retry only. A failing IPC call usually means a genuine backend
      // error rather than a transient blip worth hammering.
      retry: 1,
    },
  },
});
