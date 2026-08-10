/**
 * Start, cancel and undo controls for a cleanup run.
 */
import { Loader2, Sparkles, Undo2, XCircle } from 'lucide-react';
import type { MerchantCleanupPreview, MerchantCleanupRun } from '@/lib/ipc';
import { Button } from '@/components/ui/button';

const DANGER =
  'border-red-200 text-red-600 hover:bg-red-50 hover:border-red-300';

/** Cancels a running cleanup. */
function StopButton({ onCancel }: { onCancel: () => void }) {
  return (
    <Button
      variant="outline"
      onClick={onCancel}
      className="border-[#064E3B]/20 text-[#064E3B] hover:bg-[#064E3B]/5"
    >
      <XCircle className="w-4 h-4 mr-2" /> Stop
    </Button>
  );
}

/** Starts a cleanup run. */
function StartButton({
  preview,
  isStarting,
  isFinished,
  noModel,
  onStart,
}: {
  preview: MerchantCleanupPreview | null;
  isStarting: boolean;
  isFinished: boolean;
  noModel: boolean;
  onStart: () => void;
}) {
  const hasQueue = preview !== null && preview.candidate_count > 0;
  const cannotStart = isStarting || !hasQueue || noModel || !preview?.llm_eligible;

  return (
    <Button variant="accent" onClick={onStart} disabled={cannotStart}>
      {isStarting ? (
        <Loader2 className="w-4 h-4 mr-2 animate-spin" />
      ) : (
        <Sparkles className="w-4 h-4 mr-2" />
      )}
      {isFinished && hasQueue ? 'Continue' : 'Normalize with AI'}
    </Button>
  );
}

/** Reverts an entire completed run. */
function UndoRunButton({
  run,
  busyId,
  onUndoRun,
}: {
  run: MerchantCleanupRun;
  busyId: string | null;
  onUndoRun: (run: MerchantCleanupRun) => void;
}) {
  const isBusy = busyId === run.run_id;
  return (
    <Button variant="outline" onClick={() => onUndoRun(run)} disabled={isBusy} className={DANGER}>
      {isBusy ? (
        <Loader2 className="w-4 h-4 mr-2 animate-spin" />
      ) : (
        <Undo2 className="w-4 h-4 mr-2" />
      )}
      Undo this run
    </Button>
  );
}

interface CleanupActionButtonsProps {
  preview: MerchantCleanupPreview | null;
  isRunning: boolean;
  isStarting: boolean;
  isFinished: boolean;
  noModel: boolean;
  finishedRun: MerchantCleanupRun | undefined;
  busyId: string | null;
  onStart: () => void;
  onCancel: () => void;
  onUndoRun: (run: MerchantCleanupRun) => void;
}

/** Start, cancel and undo controls, shown per run state. */
export default function CleanupActionButtons(props: CleanupActionButtonsProps) {
  const { isRunning, finishedRun } = props;
  const canUndo = finishedRun && finishedRun.applied > 0 && !isRunning;

  return (
    <div className="flex items-center gap-2 shrink-0">
      {isRunning ? (
        <StopButton onCancel={props.onCancel} />
      ) : (
        <StartButton
          preview={props.preview}
          isStarting={props.isStarting}
          isFinished={props.isFinished}
          noModel={props.noModel}
          onStart={props.onStart}
        />
      )}

      {canUndo && (
        <UndoRunButton run={finishedRun} busyId={props.busyId} onUndoRun={props.onUndoRun} />
      )}
    </div>
  );
}
