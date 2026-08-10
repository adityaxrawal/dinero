/**
 * Detects regressions between readiness snapshots.
 */
import { ReleaseReadinessLocalMetrics } from '../../lib/ipc';

/**
 * Reports which metrics worsened between two snapshots.
 *
 * Comparing against the previous snapshot turns absolute numbers into a
 * direction of travel, which is the useful signal before a release.
 */
export function detectRegressions(
  previous: ReleaseReadinessLocalMetrics,
  current: ReleaseReadinessLocalMetrics
): Array<keyof ReleaseReadinessLocalMetrics> {
  const regressions: Array<keyof ReleaseReadinessLocalMetrics> = [];
  if (current.unresolved_clusters > previous.unresolved_clusters)
    regressions.push('unresolved_clusters');
  if (current.llm_fallback_rate > previous.llm_fallback_rate) regressions.push('llm_fallback_rate');
  if (current.statement_parse_failure_rate > previous.statement_parse_failure_rate) {
    regressions.push('statement_parse_failure_rate');
  }
  return regressions;
}
