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
import { NON_PDF_ERROR, isPdf, validateDroppedFiles, type ValidationError } from './validation';

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

  const pickAndUpload = useCallback(async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: 'PDF', extensions: ['pdf'] }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length === 0) return;

      if (paths.some((path) => !isPdf(path))) return reject(NON_PDF_ERROR);
      // The size limit is enforced by Rust before it reads a file (audit
      // N-D1); an oversize file comes back as a per-file error.
      await uploadPaths(paths);
    } catch (err) {
      toast({ variant: 'destructive', ...getErrorToast(err) });
    }
  }, [uploadPaths, toast, reject]);

  const dropFiles = useCallback(
    (files: File[]) => {
      const error = files.length > 0 ? validateDroppedFiles(files) : null;
      if (error) return reject(error);
      pickAndUpload();
    },
    [pickAndUpload, reject]
  );

  return { isUploading, batchProgress, pickAndUpload, dropFiles };
}
