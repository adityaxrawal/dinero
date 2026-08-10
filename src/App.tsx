import { useEffect } from 'react';
import { RouterProvider } from 'react-router-dom';
import { QueryClientProvider } from '@tanstack/react-query';
import { onAction } from '@tauri-apps/plugin-notification';
import { API } from './lib/ipc';
import { logger } from './lib/logger';
import { ErrorBoundary } from './components/ErrorBoundary';
import ToastProvider from '@/components/ToastProvider';
import { toast } from '@/hooks/use-toast';
import { ToastAction } from '@/components/ui/toast';
import { router } from './routes';
import { GlobalStateProvider } from './lib/GlobalStateContext';
import { queryClient } from './lib/queryClient';
import { useIpcQueryInvalidation } from './hooks/useIpcQueryInvalidation';
import { useResumeFromSleepRefetch } from './hooks/useResumeFromSleepRefetch';
import { syncLlmState } from './lib/syncLlmState';
import './App.css';

/** Payload of the backend's `update_available` event. */
interface UpdateAvailablePayload {
  version: string;
  current_version: string;
  notes: string | null;
}

/**
 * Headless component that connects backend events to frontend reactions.
 *
 * Renders nothing -- it exists purely to own effects that must live inside the
 * React tree (and inside QueryClientProvider, since cache invalidation needs
 * the client). Keeping them here rather than in App means these subscriptions
 * mount once and survive every navigation.
 */
function IpcEventBridge() {
  // Backend events invalidate the matching React Query caches.
  useIpcQueryInvalidation();
  // Refetches after the machine wakes, where cached data may be badly stale.
  useResumeFromSleepRefetch();

  // Deep links from OS notification clicks. The hash is assigned directly
  // rather than routed through the router, because this fires from outside
  // React and has no access to a navigate function.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onAction((notification) => {
      const route = notification.extra?.deep_link;
      if (typeof route === 'string') {
        window.location.hash = route;
      }
    })
      .then((handle) => {
        unlisten = () => handle.unregister();
      })
      .catch((e) => console.error('Failed to register notification action listener', e));
    return () => unlisten?.();
  }, []);

  // Update availability, surfaced as a toast with a direct install action.
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    /** Subscribes to update-available events, if the Tauri bus exists. */
    const setup = async () => {
      let listen;
      // Imported dynamically inside a try so the effect degrades quietly in a
      // plain browser or test environment, where no Tauri event bus exists.
      try {
        const m = await import('@tauri-apps/api/event');
        listen = m.listen;
      } catch {
        return;
      }
      const handle = await listen<UpdateAvailablePayload>('update_available', (event) => {
        const { version } = event.payload;
        toast({
          title: 'Update Available',
          description: `Dinero ${version} is ready to install.`,
          action: (
            <ToastAction
              altText="Update Now"
              onClick={() => {
                void API.updater
                  .confirmInstall()
                  .catch((e) => console.error('Failed to install update', e));
              }}
            >
              Update Now
            </ToastAction>
          ),
        });
      });
      unlisten = handle;
    };

    // The async setup is fired and its unlisten captured on completion; the
    // cleanup tolerates unmounting before setup resolved.
    setup().catch((e) => console.error('Failed to listen for update_available', e));
    return () => unlisten?.();
  }, []);

  return null;
}

/**
 * Root component: assembles the provider stack and the app-wide event wiring.
 *
 * Provider order below is deliberate and load-bearing. The error boundary is
 * outermost so it can catch a failure in any provider beneath it; toasts sit
 * outside React Query so an error toast can still be shown when a query
 * provider throws; and the router is innermost, since every route depends on
 * everything above it.
 */
function App() {
  // Session and navigation logging, giving backend logs the route context that
  // makes a later error entry interpretable.
  useEffect(() => {
    logger.info('Dinero Frontend Application initialized', { route: window.location.hash || '#/' });

    /** Logs each route change, giving backend logs navigation context. */
    const handleHashChange = () => {
      logger.info(`Route changed: ${window.location.hash || '#/'}`);
    };
    window.addEventListener('hashchange', handleHashChange);
    return () => window.removeEventListener('hashchange', handleHashChange);
  }, []);

  // Reconcile the saved local-LLM model against this machine's hardware. Runs
  // once at startup and is intentionally not awaited -- the UI must not block
  // on an optional subsystem.
  useEffect(() => {
    syncLlmState();
  }, []);

  return (
    <ErrorBoundary>
      <ToastProvider />
      <QueryClientProvider client={queryClient}>
        <IpcEventBridge />
        <GlobalStateProvider>
          <RouterProvider router={router} />
        </GlobalStateProvider>
      </QueryClientProvider>
    </ErrorBoundary>
  );
}

export default App;
