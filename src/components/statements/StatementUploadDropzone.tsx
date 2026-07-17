import { useCallback, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { readFile } from '@tauri-apps/plugin-fs';
import { UploadCloud } from 'lucide-react';
import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useToast } from '@/hooks/use-toast';
import { getErrorMessage } from '@/lib/getErrorMessage';
import { API } from '@/lib/ipc';
import { useGlobalState } from '@/lib/GlobalStateContext';

interface StatementUploadDropzoneProps {
  onUploaded: () => void;
  onAccessDenied: () => void;
}

// TASK-FE-012: no backend size limit exists on statements_upload -- this is
// a client-side-only "immediate feedback before even attempting the round
// trip" guard, not a server-enforced rule. 25MB comfortably exceeds any
// real bank statement PDF (typically well under 5MB).
const MAX_FILE_SIZE_BYTES = 25 * 1024 * 1024;

function isPdf(name: string, type?: string): boolean {
  return name.toLowerCase().endsWith('.pdf') || type === 'application/pdf';
}

/**
 * TASK-FE-012 (Doc 30): drag-and-drop/file-picker calling `statements_upload`,
 * with immediate validation error surfacing (non-PDF, too large, duplicate).
 * Non-PDF and too-large are checked client-side before any upload attempt;
 * duplicate detection is inherently server-side (TASK-STMT-002) — surfaced
 * distinctly from other failures as soon as the (single) round trip returns,
 * rather than lumped into a generic error message.
 *
 * Area 9 verification-pass fix: Doc 30 also asks for "a queued state for
 * items beyond the backend's 5-concurrent-parser cap (TASK-STMT-009)" —
 * the real `statement_batch_progress` event (`queues.rs`'s
 * `BatchProgressTracker`, emitted for batches over 10 files) existed with
 * zero frontend listeners anywhere, so a large batch showed a static
 * "Uploading…" spinner for the whole multi-minute duration with no
 * indication of how many statements were still waiting on the 5-permit
 * semaphore. Now shown via `GlobalStateContext`'s `batchProgress`.
 */
export default function StatementUploadDropzone({ onUploaded, onAccessDenied }: StatementUploadDropzoneProps) {
  const { toast } = useToast();
  const { batchProgress, setBatchProgress } = useGlobalState();
  const [isDragging, setIsDragging] = useState(false);
  const [isUploading, setIsUploading] = useState(false);

  const uploadPaths = useCallback(
    async (paths: string[]) => {
      setIsUploading(true);
      setBatchProgress(null);
      let accessDenied = false;
      let succeeded = 0;
      const duplicates: string[] = [];
      const otherFailures: string[] = [];

      try {
        const results = await API.statements.upload(paths);
        for (const result of results) {
          if (result.status.startsWith('error')) {
            const errMsg = result.status.replace(/^error:\s*/, '');
            if (errMsg.includes('File access denied') || errMsg.includes('Permission denied')) {
              accessDenied = true;
            } else if (errMsg.includes('duplicate')) {
              duplicates.push(result.filename || 'A file');
            } else {
              otherFailures.push(errMsg);
            }
          } else {
            succeeded += 1;
          }
        }
      } catch (err) {
        otherFailures.push(getErrorMessage(err));
      } finally {
        setIsUploading(false);
      }

      if (accessDenied) onAccessDenied();
      if (succeeded > 0) {
        toast({
          title: paths.length > 1 ? `${succeeded} of ${paths.length} Uploads Started` : 'Upload Started',
          description: 'Statement(s) are being processed.',
        });
      }
      if (duplicates.length > 0) {
        toast({
          variant: 'destructive',
          title: 'Already Uploaded',
          description: `${duplicates.slice(0, 3).join(', ')} — this statement was already imported.`,
        });
      }
      if (otherFailures.length > 0) {
        toast({ variant: 'destructive', title: 'Some Uploads Failed', description: otherFailures.slice(0, 3).join('; ') });
      }
      onUploaded();
    },
    [onUploaded, onAccessDenied, toast, setBatchProgress],
  );

  const handleFileUpload = useCallback(async () => {
    try {
      const selected = await open({ multiple: true, filters: [{ name: 'PDF', extensions: ['pdf'] }] });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      if (paths.length === 0) return;

      // Immediate client-side validation: non-PDF (belt-and-suspenders on
      // top of the file picker's own filter, since a user can still type an
      // arbitrary path) and too-large, before any upload attempt at all.
      const tooLarge: string[] = [];
      for (const path of paths) {
        if (!isPdf(path)) {
          toast({ variant: 'destructive', title: 'Upload Error', description: 'Only PDF files are allowed.' });
          return;
        }
        const bytes = await readFile(path);
        if (bytes.byteLength > MAX_FILE_SIZE_BYTES) {
          tooLarge.push(path.split(/[/\\]/).pop() || path);
        }
      }
      if (tooLarge.length > 0) {
        toast({
          variant: 'destructive',
          title: 'File Too Large',
          description: `${tooLarge.join(', ')} exceeds the 25MB limit.`,
        });
        return;
      }

      await uploadPaths(paths);
    } catch (err) {
      toast({ variant: 'destructive', title: 'Upload Failed', description: getErrorMessage(err) });
    }
  }, [uploadPaths, toast]);

  const onDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };
  const onDragLeave = () => setIsDragging(false);

  const onDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);

      const files = Array.from(e.dataTransfer.files);
      if (files.length > 0) {
        const nonPdf = files.find((file) => !isPdf(file.name, file.type));
        if (nonPdf) {
          toast({ variant: 'destructive', title: 'Upload Error', description: 'Only PDF files are allowed.' });
          return;
        }
        const tooLarge = files.find((file) => file.size > MAX_FILE_SIZE_BYTES);
        if (tooLarge) {
          toast({ variant: 'destructive', title: 'File Too Large', description: `${tooLarge.name} exceeds the 25MB limit.` });
          return;
        }
      }

      // Browser drag-drop events don't carry absolute filesystem paths (needed
      // for statements_upload's byte-read) -- fall back to the file picker,
      // which does, once the immediate client-side checks above have passed.
      handleFileUpload();
    },
    [handleFileUpload, toast],
  );

  return (
    <Card
      className={cn(
        'border-2 border-dashed transition-colors cursor-pointer',
        isDragging ? 'border-primary bg-primary/10' : 'border-border hover:border-primary/50 hover:bg-secondary/50',
      )}
      onDragOver={onDragOver}
      onDragLeave={onDragLeave}
      onDrop={onDrop}
      onClick={handleFileUpload}
      role="button"
      data-testid="dropzone"
      tabIndex={0}
      aria-label="Upload a PDF statement. Click or drag and drop."
      onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && handleFileUpload()}
    >
      <CardContent className="flex flex-col items-center justify-center py-12 text-center">
        <div className="w-16 h-16 rounded-full bg-secondary flex items-center justify-center mb-4" aria-hidden="true">
          <UploadCloud className="w-8 h-8 text-muted-foreground" />
        </div>
        <h2 className="text-lg font-semibold mb-1">Upload Statement</h2>
        <p className="text-sm text-muted-foreground mb-4" role="status">
          {isUploading
            ? 'Uploading…'
            : batchProgress
              ? `Parsing ${batchProgress.parsed} of ${batchProgress.total} statements (max 5 at a time)… ~${batchProgress.etaSeconds}s remaining`
              : 'Drag and drop your PDF statements here, or click to browse.'}
        </p>
        <Button asChild variant="secondary" aria-hidden="true" tabIndex={-1}>
          <span>Browse Files</span>
        </Button>
      </CardContent>
    </Card>
  );
}
