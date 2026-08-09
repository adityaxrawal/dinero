import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { API } from '@/lib/ipc';
import { isTauriRuntime } from '@/lib/tauriRuntime';
import { useLicenseStore } from '@/stores/useLicenseStore';

const CORRUPTION_EVENT = 'db_corrupted';

const START_FRESH_WARNING =
  'Start fresh? This permanently deletes all local data (transactions, statements, settings) and returns you to onboarding. This cannot be undone.';

/** Native dialog where available; the browser confirm is the web-preview path. */
async function askConfirm(warning: string): Promise<boolean> {
  try {
    const { ask } = await import('@tauri-apps/plugin-dialog');
    return await ask(warning, { title: 'Start Fresh', kind: 'warning' });
  } catch {
    return confirm(warning);
  }
}

export function useBackendStatus() {
  const navigate = useNavigate();
  const [backendStatus, setBackendStatus] = useState<'healthy' | 'offline' | 'corrupted'>('healthy');
  const [isRestoring, setIsRestoring] = useState(false);
  const [isStartingFresh, setIsStartingFresh] = useState(false);
  const hydrateLicenseStore = useLicenseStore((s) => s.hydrate);

  const handleRestoreBackup = useCallback(async () => {
    setIsRestoring(true);
    try {
      await API.db.restoreBackup();
      setBackendStatus('healthy');
    } catch (err) {
      console.error('Restore backup failed:', err);
    } finally {
      setIsRestoring(false);
    }
  }, []);

  const handleStartFresh = useCallback(async () => {
    if (!(await askConfirm(START_FRESH_WARNING))) return;

    setIsStartingFresh(true);
    try {
      await API.dev.resetDatabase();
      window.location.reload();
    } catch (err) {
      console.error('Start fresh failed:', err);
      setIsStartingFresh(false);
    }
  }, []);

  useEffect(() => {
    if (isTauriRuntime() && !localStorage.getItem('dinero_onboarded')) {
      navigate('/onboarding', { replace: true });
      return;
    }

    // A corrupted status must survive a later health check resolving.
    const ifNotCorrupted = (next: 'healthy' | 'offline') => (prev: typeof backendStatus) =>
      prev === 'healthy' || prev === 'offline' ? next : prev;

    API.status
      .check()
      .then((result) => setBackendStatus(ifNotCorrupted(result.status as 'healthy')))
      .catch(() => setBackendStatus(ifNotCorrupted('offline')));

    hydrateLicenseStore();

    const unlisteners: (() => void)[] = [];

    const setup = async () => {
      let listen;
      try {
        const m = await import('@tauri-apps/api/event');
        listen = m.listen;
      } catch {
        return;
      }
      if (!listen) return;

      unlisteners.push(await listen(CORRUPTION_EVENT, () => setBackendStatus('corrupted')));

      // Doc 30 TASK-RT-003: `alert_threshold_crossed` handling (toast +
      // persistent banner) lives in `useAlertStore.ts`'s own module-load
      // subscription, alongside the other event-store patterns
      // (`useSyncStore.ts`) -- it previously listened here with a fabricated
      // `{category, threshold}` payload shape that never matched the real
      // `{transaction_id, alert_type, message}` the backend emits.
    };

    setup().catch(console.error);
    return () => unlisteners.forEach((fn) => fn());
  }, [navigate, hydrateLicenseStore]);

  return {
    backendStatus,
    isRestoring,
    isStartingFresh,
    handleRestoreBackup,
    handleStartFresh,
  };
}
