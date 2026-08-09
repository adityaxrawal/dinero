import type { LlmModelInfo, MerchantCleanupPreview, MerchantCleanupRun } from '@/lib/ipc';
import CleanupAlerts from './CleanupAlerts';
import CleanupStats from './CleanupStats';
import CleanupActionCard, { type RunState } from './CleanupActionCard';
import CleanupQueue from './CleanupQueue';
import CleanupEmptyState from './CleanupEmptyState';
import PastRuns from './PastRuns';

interface CleanupBodyProps {
  preview: MerchantCleanupPreview | null;
  runs: MerchantCleanupRun[];
  activeModel: LlmModelInfo | null;
  error: string | null;
  run: RunState;
  onStart: () => void;
  onCancel: () => void;
  onUndoRun: (run: MerchantCleanupRun) => void;
  onUndoChange: (correctionId: string) => void;
}

export default function CleanupBody({
  preview,
  runs,
  activeModel,
  error,
  run,
  onStart,
  onCancel,
  onUndoRun,
  onUndoChange,
}: CleanupBodyProps) {
  const noModel = activeModel === null;
  const hasQueue = preview !== null && preview.candidate_count > 0;
  const showQueue = preview !== null && preview.by_bank.length > 0 && !run.isRunning;
  const isIdleAndClean = preview !== null && preview.candidate_count === 0 && runs.length === 0;

  return (
    <>
      <CleanupAlerts
        error={error}
        blocked={preview !== null && !preview.llm_eligible}
        noModel={noModel}
        totalRamGb={preview?.total_ram_gb}
      />

      {hasQueue && <CleanupStats preview={preview} activeModel={activeModel} />}

      <CleanupActionCard
        preview={preview}
        activeModel={activeModel}
        noModel={noModel}
        run={run}
        onStart={onStart}
        onCancel={onCancel}
        onUndoRun={onUndoRun}
      />

      {showQueue && <CleanupQueue preview={preview} />}

      {runs.length > 0 && (
        <PastRuns
          runs={runs}
          onUndoRun={onUndoRun}
          onUndoChange={onUndoChange}
          busyId={run.busyId}
        />
      )}

      {isIdleAndClean && <CleanupEmptyState />}
    </>
  );
}
