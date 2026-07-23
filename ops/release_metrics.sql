-- Doc 30 TASK-OPS-009: reference queries backing the Release Readiness
-- debug view's "locally-verifiable metrics" section
-- (src/components/debug/ReleaseReadinessViewer.tsx,
-- src-tauri/src/commands/release_readiness.rs). These are the same queries
-- `release_readiness.rs::compute_local_metrics` runs against the live
-- SQLCipher connection -- kept here as a standalone, reviewable reference
-- (same convention as `security_hardening_check.sh` being a real script
-- rather than only inline logic), not a separate execution path.
--
-- Scan speed, extraction accuracy, false-positive/false-merge rate, PDF
-- parse accuracy, and alert latency are deliberately NOT queried here --
-- there is no ground-truth label in a real user's own database to compare
-- against, so these are measured by the test suite against a labeled
-- benchmark corpus instead (phase9/phase10 rigorous tests,
-- `reconciliation_regression.rs`, `event_load_test.rs`'s p95 latency check)
-- and surfaced via `scripts/verify_acceptance_criteria.py --output <path>`,
-- not by querying live user data that has no known-correct answer to
-- compare against.

-- Unresolved reconciliation clusters (a rising count across releases is a
-- reconciliation-quality regression signal).
SELECT count(*) AS unresolved_clusters
FROM reconciliation_clusters
WHERE cluster_status IN ('open', 'deferred');

-- LLM extraction fallback rate (rising across releases suggests the
-- deterministic ladder is regressing and leaning harder on the LLM tier).
SELECT
  CAST(SUM(CASE WHEN extraction_method = 'llm' THEN 1 ELSE 0 END) AS REAL)
    / NULLIF(COUNT(*), 0) AS llm_fallback_rate
FROM transaction_observations;

-- Local database size (a proxy for whether retention/vacuum policies —
-- TASK-DB-019 — are keeping up as the install ages).
SELECT page_count * page_size AS db_size_bytes
FROM pragma_page_count(), pragma_page_size();

-- Statement parse failure rate over the last 30 days (a coarse local proxy
-- for PDF parse accuracy -- not a substitute for the labeled benchmark
-- corpus test, but a real signal for THIS install's own recent statements).
-- Parse failures never reach `statements` at all -- they live in
-- `unprocessed_statements` until resolved (`resolved_statement_id` set) or
-- dismissed, so the rate is computed against that table, not `statements`.
SELECT
  CAST(COUNT(*) AS REAL)
    / NULLIF((SELECT COUNT(*) FROM statements WHERE created_at >= datetime('now', '-30 days')) + COUNT(*), 0)
    AS statement_parse_failure_rate
FROM unprocessed_statements
WHERE created_at >= datetime('now', '-30 days') AND resolved_statement_id IS NULL;
