import { useState } from 'react';
import { UploadCloud } from 'lucide-react';
import { Card, CardContent } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { useStatementUpload } from './upload/useStatementUpload';

interface StatementUploadDropzoneProps {
  onUploaded: () => void;
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
export default function StatementUploadDropzone({ onUploaded }: StatementUploadDropzoneProps) {
  const { isUploading, batchProgress, pickAndUpload, dropFiles } = useStatementUpload(onUploaded);
  const [isDragging, setIsDragging] = useState(false);

  const status = isUploading
    ? 'Uploading…'
    : batchProgress
      ? `Parsing ${batchProgress.parsed} of ${batchProgress.total} statements (max 5 at a time)… ~${batchProgress.etaSeconds}s remaining`
      : 'Drag and drop your PDF statements here, or click to browse.';

  return (
    <Card
      className={cn(
        'border-2 border-dashed transition-colors cursor-pointer',
        isDragging
          ? 'border-primary bg-primary/10'
          : 'border-border hover:border-primary/50 hover:bg-secondary/50'
      )}
      onDragOver={(e) => {
        e.preventDefault();
        setIsDragging(true);
      }}
      onDragLeave={() => setIsDragging(false)}
      onDrop={(e) => {
        e.preventDefault();
        setIsDragging(false);
        dropFiles(Array.from(e.dataTransfer.files));
      }}
      onClick={pickAndUpload}
      role="button"
      data-testid="dropzone"
      tabIndex={0}
      aria-label="Upload a PDF statement. Click or drag and drop."
      onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && pickAndUpload()}
    >
      <CardContent className="flex flex-col items-center justify-center py-12 text-center">
        <div
          className="w-16 h-16 rounded-full bg-secondary flex items-center justify-center mb-4"
          aria-hidden="true"
        >
          <UploadCloud className="w-8 h-8 text-muted-foreground" />
        </div>
        <h2 className="text-lg font-semibold mb-1">Upload Statement</h2>
        <p className="text-sm text-muted-foreground mb-4" role="status">
          {status}
        </p>
        <Button asChild variant="secondary" aria-hidden="true" tabIndex={-1}>
          <span>Browse Files</span>
        </Button>
      </CardContent>
    </Card>
  );
}
