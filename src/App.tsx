import { useEffect } from 'react';
import { RouterProvider } from 'react-router-dom';
import { QueryClientProvider } from '@tanstack/react-query';
import { onAction } from '@tauri-apps/plugin-notification';
import { API } from './lib/ipc';
import { ErrorBoundary } from './components/ErrorBoundary';
import ToastProvider from '@/components/ToastProvider';
import { toast } from '@/hooks/use-toast';
import { ToastAction } from '@/components/ui/toast';
import { router } from './routes';
import { GlobalStateProvider } from './lib/GlobalStateContext';
import { queryClient } from './lib/queryClient';
import { useIpcQueryInvalidation } from './hooks/useIpcQueryInvalidation';
import { useResumeFromSleepRefetch } from './hooks/useResumeFromSleepRefetch';
import './App.css';

interface UpdateAvailablePayload {
  version: string;
  current_version: string;
  notes: string | null;
}

// TASK-FE-003: mounted once, inside QueryClientProvider, so
// useIpcQueryInvalidation can reach the client via useQueryClient(). Renders
// nothing — a component is needed only because a hook can't be called from
// the same component that creates the provider wrapping it.
function IpcEventBridge() {
  useIpcQueryInvalidation();
  useResumeFromSleepRefetch();

  useEffect(() => {
    // TASK-DESK-002: clicking a native notification foregrounds the app and
    // deep-links to the relevant view. Uses `window.location.hash` directly
    // rather than `useNavigate()` -- this component is mounted outside
    // `RouterProvider` (see below), so no router context is available here,
    // but the hash router (Doc 30 TASK-FE-001) picks up a hash change from
    // anywhere, React or not.
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

  useEffect(() => {
    // TASK-DESK-005: a non-intrusive "Update Now" / "Remind Me Later"
    // prompt -- never a forced-restart popup. Implemented as a toast with
    // a single action button (shadcn's Toast only supports one): clicking
    // it installs; not clicking it *is* "Remind Me Later" -- the next
    // scheduled check (~6h) or manual menu trigger re-prompts if the
    // update is still available, so no separate snooze state is needed.
    let unlisten: (() => void) | undefined;

    const setup = async () => {
      let listen;
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
                void API.updater.confirmInstall().catch((e) =>
                  console.error('Failed to install update', e),
                );
              }}
            >
              Update Now
            </ToastAction>
          ),
        });
      });
      unlisten = handle;
    };

    setup().catch((e) => console.error('Failed to listen for update_available', e));
    return () => unlisten?.();
  }, []);

  return null;
}

function App() {
  useEffect(() => {
    // Doc 16 §12.3: the 5-tier model catalog is the single source of truth
    // for RAM requirements — never a hardcoded model id/threshold here.
    const checkRam = async () => {
      try {
        const [ramGb, models] = await Promise.all([
          API.dev.checkSystemRam(),
          API.llm.getAvailableModels(),
        ]);
        if (models.length === 0) return;

        const selectedId = localStorage.getItem('llm_model') || models[0].id;
        const selected = models.find((m) => m.id === selectedId);
        const override = localStorage.getItem('llm_ram_override') === 'true';

        if (selected && ramGb < selected.min_ram_gb && !override) {
          if (
            window.confirm(
              `Warning: Your system has ${ramGb.toFixed(1)}GB of RAM, but ${selected.name} requires at least ${selected.min_ram_gb}GB for optimal performance. You may experience slow downs or crashes.\n\nDo you want to continue anyway (allow override)?`,
            )
          ) {
            localStorage.setItem('llm_ram_override', 'true');
          } else {
            // Fall back to the lowest-tier (broadest-compatibility) model.
            const fallback = models.reduce((a, b) => (a.min_ram_gb <= b.min_ram_gb ? a : b));
            localStorage.setItem('llm_model', fallback.id);
            alert(
              `Model automatically switched to a lighter version (${fallback.name}). You can change this in Settings.`,
            );
          }
        }
      } catch (e) {
        console.error('Failed to check RAM', e);
      }
    };
    checkRam();
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
