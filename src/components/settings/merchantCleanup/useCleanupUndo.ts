/**
 * Reverts an individual correction or a whole run.
 */
import { useState } from 'react';
import { API, type MerchantCleanupProgress, type MerchantCleanupRun } from '@/lib/ipc';
import { toast } from '@/hooks/use-toast';
import { errorMessage } from '@/lib/utils';

interface UseCleanupUndoArgs {
  setError: (message: string | null) => void;
  setProgress: (progress: MerchantCleanupProgress | null) => void;
  reload: () => void;
}

/** Reverts individual corrections or whole runs. */
export function useCleanupUndo({ setError, setProgress, reload }: UseCleanupUndoArgs) {
  const [busyId, setBusyId] = useState<string | null>(null);

  /** Reverts every correction from a run. */
  const revertRun = async (run: MerchantCleanupRun) => {
    setBusyId(run.run_id);
    setError(null);
    try {
      const n = await API.merchantCleanup.revert(run.run_id);
      reload();
      setProgress(null);
      toast({
        title: `Undid ${n} correction${n === 1 ? '' : 's'}`,
        description:
          'Every merchant name, category and entity link that run changed is back, and the rules it taught are retired.',
      });
    } catch (err: unknown) {
      setError(errorMessage(err));
    } finally {
      setBusyId(null);
    }
  };

  /** Reverts one correction. */
  const revertChange = async (correctionId: string) => {
    setBusyId(correctionId);
    setError(null);
    try {
      await API.merchantCleanup.revertCorrection(correctionId);
      reload();
      toast({ title: 'Correction undone' });
    } catch (err: unknown) {
      setError(errorMessage(err));
    } finally {
      setBusyId(null);
    }
  };

  return { busyId, revertRun, revertChange };
}
