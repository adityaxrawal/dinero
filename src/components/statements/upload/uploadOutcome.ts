import { API } from '@/lib/ipc';

type UploadResult = Awaited<ReturnType<typeof API.statements.upload>>[number];

export interface UploadOutcome {
  /** macOS TCC blocked at least one file. */
  accessDenied: boolean;
  succeeded: number;
  duplicates: string[];
  otherFailures: string[];
  /** Statement ids that will stage under the same id, so their draft can be
   *  watched and the review modal auto-opened. */
  queuedIds: string[];
}

export function emptyOutcome(): UploadOutcome {
  return { accessDenied: false, succeeded: 0, duplicates: [], otherFailures: [], queuedIds: [] };
}

/** Sorts one upload round trip's per-file results into the four things the
 *  user needs told apart: blocked, succeeded, already-imported, broken. */
export function classifyUploadResults(results: UploadResult[]): UploadOutcome {
  const outcome = emptyOutcome();

  for (const result of results) {
    if (result.status === 'queued' && result.statement_id) {
      outcome.queuedIds.push(result.statement_id);
    }
    if (!result.status.startsWith('error')) {
      outcome.succeeded += 1;
      continue;
    }
    const message = result.status.replace(/^error:\s*/, '');
    if (message.includes('File access denied') || message.includes('Permission denied')) {
      outcome.accessDenied = true;
    } else if (message.includes('duplicate')) {
      outcome.duplicates.push(result.filename || 'A file');
    } else {
      outcome.otherFailures.push(message);
    }
  }

  return outcome;
}

interface ToastSpec {
  variant?: 'destructive';
  title: string;
  description: string;
}

/** One toast per distinct outcome, so a mixed batch reports all of them. */
export function uploadToasts(outcome: UploadOutcome, attempted: number): ToastSpec[] {
  const toasts: ToastSpec[] = [];

  // TASK-DESK-004 (Doc 30): a dismissable toast for this specific upload
  // attempt, not a blocking modal -- macOS TCC file/folder access is a
  // soft-fail, unlike the Keychain hard-fail case.
  if (outcome.accessDenied) {
    toasts.push({
      variant: 'destructive',
      title: 'File Access Denied',
      description:
        'macOS blocked access to this file. Grant Dinero permission in System Settings > Privacy & Security > Files and Folders.',
    });
  }
  if (outcome.succeeded > 0) {
    toasts.push({
      title:
        attempted > 1 ? `${outcome.succeeded} of ${attempted} Uploads Started` : 'Upload Started',
      description: 'Statement(s) are being processed.',
    });
  }
  if (outcome.duplicates.length > 0) {
    toasts.push({
      variant: 'destructive',
      title: 'Already Uploaded',
      description: `${outcome.duplicates.slice(0, 3).join(', ')} — this statement was already imported.`,
    });
  }
  if (outcome.otherFailures.length > 0) {
    toasts.push({
      variant: 'destructive',
      title: 'Some Uploads Failed',
      description: outcome.otherFailures.slice(0, 3).join('; '),
    });
  }

  return toasts;
}
