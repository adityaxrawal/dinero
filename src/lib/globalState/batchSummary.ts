/**
 * Accumulates the result of a multi-file statement import and turns it into a
 * single closing notification.
 *
 * Importing a batch produces one backend event per file. Rather than toasting
 * each one and burying the user under notifications, the outcome is tallied as
 * events arrive and summarised once at the end.
 */

/** Running tally for an in-flight batch. */
export interface BatchOutcome {
  succeeded: number;
  failed: number;
  /** Collected in arrival order; only the first is surfaced in the summary. */
  failureReasons: string[];
}

/**
 * A fresh zeroed tally.
 *
 * A factory rather than a shared constant, since the returned object is mutated
 * as the batch progresses and a shared instance would leak between runs.
 */
export const emptyBatchOutcome = (): BatchOutcome => ({
  succeeded: 0,
  failed: 0,
  failureReasons: [],
});

/** Toast payload shape, matching what the app's toast primitive accepts. */
interface BatchToast {
  title: string;
  description: string;
  variant: 'destructive' | 'default';
}

/**
 * Build the end-of-batch toast, or null when there is nothing worth reporting.
 *
 * Returning null for an untouched tally is what prevents a spurious toast when
 * an import is cancelled before any file was processed. `total` is passed
 * separately because it is the number of files the user selected, which stays
 * larger than succeeded + failed while the batch is still running.
 */
export function batchSummaryToast(
  outcome: BatchOutcome | null,
  total: number
): BatchToast | null {
  if (!outcome || (outcome.succeeded === 0 && outcome.failed === 0)) return null;

  // Only the first failure reason is shown. Several files usually fail for the
  // same underlying cause, and a toast is the wrong surface for a full list --
  // the detailed per-file errors remain available on the statements screen.
  const reason = outcome.failureReasons[0];
  const suffix = reason ? ` (${reason})` : '';

  return {
    title: 'Batch Import Complete',
    // Partial success still reports the successes, so the user can see what did
    // land rather than only what broke.
    description:
      outcome.failed > 0
        ? `${outcome.succeeded}/${total} imported, ${outcome.failed} failed${suffix}.`
        : `${outcome.succeeded}/${total} imported.`,
    variant: outcome.failed > 0 ? 'destructive' : 'default',
  };
}
