import { ReleaseReadinessLocalMetrics } from '../../lib/ipc';

/**
 * Doc 30 TASK-OPS-009 acceptance `test_trend_view_highlights_regressions`.
 * Mirrors `release_readiness::detect_regressions` (src-tauri) field-for-field:
 * a metric "regresses" if it gets worse release-over-release.
 * `db_size_bytes` growing is expected as an install ages and is never
 * flagged as a regression.
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
