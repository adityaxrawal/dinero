import { useCallback, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { readFile } from '@tauri-apps/plugin-fs';
import { API } from '@/lib/ipc';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage, getErrorToast } from '@/lib/errorMapping';
import { useGlobalState } from '@/lib/GlobalStateContext';
import { classifyUploadResults, emptyOutcome, uploadToasts } from './uploadOutcome';
import {
  MAX_FILE_SIZE_BYTES,
  NON_PDF_ERROR,
  basename,
  isPdf,
  tooLargeError,
  validateDroppedFiles,
  type ValidationError,
} from './validation';

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
        // A 'queued' upload eventually stages under this same statement_id
        // (stage_parse_pipeline reuses insert_queued()'s pre-minted id as the
        // draft id) — watching it is what lets the resulting statement_staged
        // event auto-open the review modal for this user-initiated upload.
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

  /** Immediate client-side validation before any upload attempt: non-PDF
   *  (belt-and-suspenders on the picker's own filter, since a user can still
   *  type an arbitrary path) and too-large. */
  const pickAndUpload = useCallback(async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: 'PDF', extensions: ['pdf'] }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length === 0) return;

      const tooLarge: string[] = [];
      for (const path of paths) {
        if (!isPdf(path)) return reject(NON_PDF_ERROR);
        const bytes = await readFile(path);
        if (bytes.byteLength > MAX_FILE_SIZE_BYTES) tooLarge.push(basename(path));
      }
      if (tooLarge.length > 0) return reject(tooLargeError(tooLarge));

      await uploadPaths(paths);
    } catch (err) {
      toast({ variant: 'destructive', ...getErrorToast(err) });
    }
  }, [uploadPaths, toast, reject]);

  const dropFiles = useCallback(
    (files: File[]) => {
      const error = files.length > 0 ? validateDroppedFiles(files) : null;
      if (error) return reject(error);
      // Browser drag-drop events don't carry absolute filesystem paths (needed
      // for statements_upload's byte-read) -- fall back to the file picker,
      // which does, once the client-side checks above have passed.
      pickAndUpload();
    },
    [pickAndUpload, reject]
  );

  return { isUploading, batchProgress, pickAndUpload, dropFiles };
}
