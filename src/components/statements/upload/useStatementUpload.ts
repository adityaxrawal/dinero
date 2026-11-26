/**
 * Handles selecting and uploading statement files, including batches.
 */
import { useCallback, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { API } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage, getErrorToast } from '@/lib/errorMapping';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { classifyUploadResults, emptyOutcome, uploadToasts } from './uploadOutcome';
import { NON_PDF_ERROR, isPdf, type ValidationError } from './validation';

/** Handles selecting and uploading statement files, including batches. */
export function useStatementUpload(onUploaded: () => void) {
  const { toast } = useToast();
  const { batchProgress, setBatchProgress, watchDraftOrigin } = useGlobalState();
  const [isUploading, setIsUploading] = useState(false);

  const reject = useCallback(
    (error: ValidationError) => toast({ variant: 'destructive', ...error }),
    [toast]
  );

  const uploadPaths = useCallback(
    async (paths: string[]) => {
      setIsUploading(true);
      setBatchProgress(null);
      let outcome = emptyOutcome();
      try {
        const results = await API.statements.upload(paths);
        outcome = classifyUploadResults(results);
        outcome.queuedIds.forEach((id) => watchDraftOrigin(id));
      } catch (err) {
        outcome.otherFailures.push(getErrorMessage(err));
      } finally {
        setIsUploading(false);
      }

      for (const spec of uploadToasts(outcome, paths.length)) toast(spec);
      onUploaded();
    },
    [onUploaded, toast, setBatchProgress, watchDraftOrigin]
  );

  // Picked and dropped files take the same path. The size limit is enforced
  // by Rust before it reads a file (audit N-D1); an oversize file comes back
  // as a per-file error.
  const uploadChecked = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      if (paths.some((path) => !isPdf(path))) return reject(NON_PDF_ERROR);
      await uploadPaths(paths);
    },
    [uploadPaths, reject]
  );

  const pickAndUpload = useCallback(async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: 'PDF', extensions: ['pdf'] }],
      });
      if (!selected) return;
      await uploadChecked(Array.isArray(selected) ? selected : [selected]);
    } catch (err) {
      toast({ variant: 'destructive', ...getErrorToast(err) });
    }
  }, [uploadChecked, toast]);

  /** Uploads paths from the native drag-drop event (audit F8-3). */
  const dropFiles = uploadChecked;

  return { isUploading, batchProgress, pickAndUpload, dropFiles };
}
