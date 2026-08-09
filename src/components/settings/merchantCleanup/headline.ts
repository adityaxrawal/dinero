import type { MerchantCleanupPreview, MerchantCleanupProgress } from '@/lib/ipc';

export interface HeadlineState {
  preview: MerchantCleanupPreview | null;
  progress: MerchantCleanupProgress | null;
  isRunning: boolean;
  isFinished: boolean;
}

const FINISHED_TITLE: Record<string, string> = {
  cancelled: 'Stopped early — the fixes so far are kept',
  failed: 'Run failed',
};

/** What state the run is in, in one line. */
export function headlineTitle({ preview, progress, isRunning, isFinished }: HeadlineState): string {
  if (preview === null) return 'Checking your transactions…';
  if (isRunning) {
    return progress?.bank_name
      ? `Reading ${progress.bank_name} alerts…`
      : 'Starting the on-device AI…';
  }
  if (isFinished && progress) {
    return FINISHED_TITLE[progress.status] ?? 'Cleanup finished';
  }
  if (preview.candidate_count === 0) return 'Every merchant name looks good';
  const plural = preview.candidate_count === 1 ? '' : 's';
  return `${preview.candidate_count} merchant name${plural} Dinero isn't sure about`;
}

/** The sentence under the title: what just happened, and what is left. */
export function headlineBlurb({ preview, progress, isRunning, isFinished }: HeadlineState): string {
  if (isRunning) return 'Worst first — stop any time and everything fixed so far is kept.';

  const queued = preview && preview.candidate_count > 0;

  if (isFinished && progress) {
    const skipped = progress.skipped > 0 ? `, ${progress.skipped} skipped` : '';
    const left = queued ? `${preview.candidate_count} still to go.` : 'Nothing left in the queue.';
    return `Fixed ${progress.applied} of ${progress.processed} read${skipped}. ${left}`;
  }
  if (queued) {
    return 'These came out of the email as a gateway code or a fragment of a sentence. Worst first, so you can stop early and keep the fixes.';
  }
  return 'Nothing scored below the confidence threshold.';
}
