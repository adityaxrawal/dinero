-- TASK-DB-011: Document 18 §4.5 specifies CHECK constraints on `decision`
-- (7 values: auto_matched_exact/auto_matched_scored/ambiguous_pending/
-- new_canonical/manually_confirmed/manually_corrected/rejected_match) and
-- `review_status` (not_required/pending_review/reviewed) -- neither existed.
-- Checked every real usage first (learning from TASK-DB-008/009): production
-- code (reconciliation/audit.rs) already used the correct 7 `decision`
-- values via its `DecisionType` enum, but used "unreviewed"/"needs_review"
-- for review_status instead of Document 18's "not_required"/"pending_review"
-- -- fixed in a companion change (zero other code read those exact
-- strings, confirmed before renaming).
--
-- Also adds the two indexes Document 18 §6 specifies ("Audit and lookup").
-- Document 18 §4.5 names these columns `incoming_observation_id`/
-- `matched_canonical_transaction_id`; the already-built schema and every
-- Rust call site instead use `observation_id`/`matched_transaction_id`
-- consistently throughout -- a naming-only difference, not fixed here
-- (renaming would touch many already-correct call sites for zero
-- functional benefit; flagged as an accepted, self-consistent deviation).
CREATE TABLE match_decisions_new (
    id TEXT PRIMARY KEY,
    observation_id TEXT REFERENCES transaction_observations(id) ON DELETE RESTRICT,
    matched_transaction_id TEXT REFERENCES transactions(id) ON DELETE RESTRICT,
    decision TEXT CHECK(decision IN (
        'auto_matched_exact', 'auto_matched_scored', 'ambiguous_pending',
        'new_canonical', 'manually_confirmed', 'manually_corrected', 'rejected_match'
    )),
    score NUMERIC,
    rules_triggered_json JSONB,
    review_status TEXT CHECK(review_status IN ('not_required', 'pending_review', 'reviewed')),
    reviewed_by TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO match_decisions_new SELECT * FROM match_decisions;
DROP TABLE match_decisions;
ALTER TABLE match_decisions_new RENAME TO match_decisions;

CREATE TRIGGER IF NOT EXISTS immutable_match_decisions
BEFORE UPDATE ON match_decisions
BEGIN
    SELECT RAISE(ABORT, 'match_decisions is immutable');
END;

CREATE INDEX IF NOT EXISTS idx_match_decisions_observation ON match_decisions(observation_id);
CREATE INDEX IF NOT EXISTS idx_match_decisions_matched_transaction ON match_decisions(matched_transaction_id);
