-- TASK-DB-009: Document 18 §4.7 requires a `source_type` column
-- (gmail_email / manual_upload) that never existed anywhere in the
-- migrated schema -- without it the UI cannot filter/display statements
-- by entry point (Document 11 §17), only infer it from
-- `source_message_id` nullability, which Document 18 explicitly says not
-- to rely on. Also adds the `parse_status` CHECK Document 18 §4.7
-- specifies (queued/processing/parsed/failed -- Document 18's authoritative
-- values, not Document 30's own differently-worded paraphrase
-- "pending/parsed/failed/duplicate") and the (billing_period_start,
-- billing_period_end, instrument_id) index Document 18 §6 calls for
-- (duplicate-cycle detection, TASK-STMT-002).
--
-- Checked every real parse_status value already in use first: production
-- code (statements/duplicate_check.rs, statements/metadata_extractor.rs)
-- only ever writes 'parsed'; two test fixtures used 'success' (fixed in a
-- companion change to 'parsed', matching real semantics) before this
-- constraint could safely land.
CREATE TABLE statements_new (
    id TEXT PRIMARY KEY,
    instrument_id TEXT REFERENCES instruments(id),
    statement_type TEXT NOT NULL,
    source_type TEXT CHECK(source_type IN ('gmail_email', 'manual_upload')),
    billing_period_start DATE NOT NULL,
    billing_period_end DATE NOT NULL,
    due_date DATE,
    statement_date DATE,
    current_balance BIGINT,
    minimum_due BIGINT,
    rewards_summary_json JSONB,
    source_message_id TEXT,
    parse_status TEXT NOT NULL CHECK(parse_status IN ('queued', 'processing', 'parsed', 'failed')),
    is_duplicate BOOLEAN NOT NULL DEFAULT 0,
    file_hash TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO statements_new (
    id, instrument_id, statement_type, billing_period_start, billing_period_end,
    due_date, statement_date, current_balance, minimum_due, rewards_summary_json,
    source_message_id, parse_status, is_duplicate, file_hash, created_at, updated_at
)
SELECT
    id, instrument_id, statement_type, billing_period_start, billing_period_end,
    due_date, statement_date, current_balance, minimum_due, rewards_summary_json,
    source_message_id, parse_status, is_duplicate, file_hash, created_at, updated_at
FROM statements;
DROP TABLE statements;
ALTER TABLE statements_new RENAME TO statements;

CREATE INDEX IF NOT EXISTS idx_statements_billing_period_instrument
    ON statements(billing_period_start, billing_period_end, instrument_id);
