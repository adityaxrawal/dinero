import { useState, useCallback } from 'react';
import { Sparkles, Undo2 } from 'lucide-react';
import type { MerchantCleanupRun } from '@/lib/ipc';
import { ConfirmDialog } from './SettingsPrimitives';
import SectionHeading from './SectionHeading';
import { useCleanupPreview } from './merchantCleanup/useCleanupPreview';
import { useCleanupProgress } from './merchantCleanup/useCleanupProgress';
import { useCleanupStart } from './merchantCleanup/useCleanupStart';
import { useCleanupUndo } from './merchantCleanup/useCleanupUndo';
import CleanupBody from './merchantCleanup/CleanupBody';

const BLURB =
  'Some transactions end up with a merchant name the parser guessed badly — a truncated brand, a payment gateway, or a fragment of the email. This hands those to the on-device AI, which reads the original email and fills in the real merchant name and a category. It also teaches the parser, so the next scan gets that email shape right on its own. Nothing leaves your Mac, and every change can be undone.';

/**
 * Issue #12: "Normalize with LLM".
 *
 * Surfaces the transactions whose merchant name the parser is least sure
 * about and lets the user hand them to the on-device model, which reads the
 * original email and returns the real merchant plus a category.
 *
 * Three things this panel has to answer at all times, because a run is long and
 * silent: what state is it in, is it actually working, and how do I take it
 * back. So the run reports measured rate and ETA rather than a static estimate,
 * shows the model's answers as they land rather than only a counter, and reads
 * its undo affordance out of the database — `merchant_llm_corrections` is the
 * run record, so a window reload can no longer strand a revertible run.
 */
export default function MerchantCleanupSettings() {
  const [pendingRun, setPendingRun] = useState<MerchantCleanupRun | null>(null);

  const { preview, runs, activeModel, error, setError, loadPreview, loadRuns } =
    useCleanupPreview();

  // The queue is derived from confidence, so refreshing after a run shows
  // exactly what is left rather than a stale count.
  const reload = useCallback(() => {
    loadPreview();
    loadRuns();
  }, [loadPreview, loadRuns]);

  const { progress, setProgress, feed, setFeed, setStartedAt, isRunning, live, pct, isFinished } =
    useCleanupProgress({ running: preview?.running === true, onRunEnd: reload });

  const { isStarting, handleStart, handleCancel } = useCleanupStart({
    preview,
    setError,
    setProgress,
    setFeed,
    setStartedAt,
  });

  const { busyId, revertRun, revertChange } = useCleanupUndo({ setError, setProgress, reload });

  const finishedRun =
    isFinished && progress ? runs.find((r) => r.run_id === progress.run_id) : undefined;

  return (
    <section>
      <SectionHeading icon={Sparkles} title="Merchant Names & Categories" description={BLURB} />

      <CleanupBody
        preview={preview}
        runs={runs}
        activeModel={activeModel}
        error={error}
        run={{ progress, feed, live, pct, isRunning, isFinished, isStarting, finishedRun, busyId }}
        onStart={handleStart}
        onCancel={handleCancel}
        onUndoRun={setPendingRun}
        onUndoChange={revertChange}
      />

      <ConfirmDialog
        open={pendingRun !== null}
        onOpenChange={(open) => !open && setPendingRun(null)}
        icon={<Undo2 className="w-5 h-5" aria-hidden="true" />}
        title="Undo this cleanup run?"
        description={
          pendingRun
            ? `Every merchant name, category and entity link those ${pendingRun.applied} correction${pendingRun.applied === 1 ? '' : 's'} changed goes back to what it was, and the extraction rules the run learned are retired.`
            : ''
        }
        confirmLabel="Undo run"
        onConfirm={() => pendingRun && void revertRun(pendingRun)}
      />
    </section>
  );
}
