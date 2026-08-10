/**
 * Polls backend health and exposes the recovery actions for a corrupted database.
 */
import { useState, useEffect, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { API } from '@/lib/ipc';
import { isTauriRuntime } from '@/lib/tauriRuntime';
import { useLicenseStore } from '@/stores/useLicenseStore';

const CORRUPTION_EVENT = 'db_corrupted';

const START_FRESH_WARNING =
  'Start fresh? This permanently deletes all local data (transactions, statements, settings) and returns you to onboarding. This cannot be undone.';

/** Confirms a destructive recovery choice before acting. */
async function askConfirm(warning: string): Promise<boolean> {
  try {
    const { ask } = await import('@tauri-apps/plugin-dialog');
    return await ask(warning, { title: 'Start Fresh', kind: 'warning' });
  } catch {
    return confirm(warning);
  }
}

/** Polls backend health and exposes the corrupted-database recovery actions. */
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

    /** Runs an action only while the database is usable. */
    const ifNotCorrupted = (next: 'healthy' | 'offline') => (prev: typeof backendStatus) =>
      prev === 'healthy' || prev === 'offline' ? next : prev;

    API.status
      .check()
      .then((result) => setBackendStatus(ifNotCorrupted(result.status as 'healthy')))
      .catch(() => setBackendStatus(ifNotCorrupted('offline')));

    hydrateLicenseStore();

    const unlisteners: (() => void)[] = [];

    /** Starts the health poll. */
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
