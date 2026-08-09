export interface BatchOutcome {
  succeeded: number;
  failed: number;
  failureReasons: string[];
}

export const emptyBatchOutcome = (): BatchOutcome => ({
  succeeded: 0,
  failed: 0,
  failureReasons: [],
});

interface BatchToast {
  title: string;
  description: string;
  variant: 'destructive' | 'default';
}

/**
 * Doc 30 TASK-RT-008 `test_batch_upload_aggregates_into_single_summary`: one
 * aggregate toast on batch completion ("8/10 imported, 2 failed due to
 * password") instead of one toast per file. Returns null when the batch
 * produced no outcomes worth reporting.
 */
export function batchSummaryToast(
  outcome: BatchOutcome | null,
  total: number
): BatchToast | null {
  if (!outcome || (outcome.succeeded === 0 && outcome.failed === 0)) return null;

  const reason = outcome.failureReasons[0];
  const suffix = reason ? ` (${reason})` : '';

  return {
    title: 'Batch Import Complete',
    description:
      outcome.failed > 0
        ? `${outcome.succeeded}/${total} imported, ${outcome.failed} failed${suffix}.`
        : `${outcome.succeeded}/${total} imported.`,
    variant: outcome.failed > 0 ? 'destructive' : 'default',
  };
}
