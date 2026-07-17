import { useEffect } from 'react';
import { RouterProvider } from 'react-router-dom';
import { QueryClientProvider } from '@tanstack/react-query';
import { API } from './lib/ipc';
import { ErrorBoundary } from './components/ErrorBoundary';
import ToastProvider from '@/components/ToastProvider';
import { router } from './routes';
import { GlobalStateProvider } from './lib/GlobalStateContext';
import { queryClient } from './lib/queryClient';
import { useIpcQueryInvalidation } from './hooks/useIpcQueryInvalidation';
import './App.css';

// TASK-FE-003: mounted once, inside QueryClientProvider, so
// useIpcQueryInvalidation can reach the client via useQueryClient(). Renders
// nothing — a component is needed only because a hook can't be called from
// the same component that creates the provider wrapping it.
function IpcEventBridge() {
  useIpcQueryInvalidation();
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
