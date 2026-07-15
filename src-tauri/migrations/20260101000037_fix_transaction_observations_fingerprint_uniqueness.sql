-- TASK-TXN-008/009: migration 024 added `UNIQUE(fingerprint)` alongside the
-- spec'd `UNIQUE(source_pipeline, source_record_id)`, but Document 18 §6's
-- own index table lists `transaction_observations` as
-- `UNIQUE(source_pipeline, source_record_id)` `(fingerprint)` `(reference_id)`
-- -- only the first pair is UNIQUE; `fingerprint` and `reference_id` are
-- meant to be plain indexes for fast lookup, not uniqueness constraints.
--
-- This was silently harmless as long as `fingerprint` was buggy and unique
-- per-message by construction (fixed in TASK-TXN-008). Now that two
-- independent observations of the *same* real transaction are meant to
-- share a fingerprint -- that's the entire mechanism the deduplication
-- engine (Area 7, TASK-DEDUP-001's "indexed lookup via
-- idx_transaction_observations_fingerprint") depends on to find candidates
-- -- a UNIQUE constraint on it would hard-fail the second insert instead of
-- creating a real duplicate observation for reconciliation to evaluate,
-- silently dropping the second source's transaction entirely.
--
-- SQLite has no ALTER TABLE ... DROP CONSTRAINT, so this uses the same
-- create-copy-drop-rename pattern already established in prior migrations.
CREATE TABLE transaction_observations_new (
    id TEXT PRIMARY KEY,
    canonical_transaction_id TEXT REFERENCES transactions(id),
    source_pipeline TEXT CHECK(source_pipeline IN ('gmail_transaction', 'statement_pdf', 'manual')),
    source_record_id TEXT,
    source_message_id TEXT,
    source_thread_id TEXT,
    statement_id TEXT REFERENCES statements(id),
    statement_entry_id TEXT REFERENCES statement_entries(id),
    instrument_id TEXT REFERENCES instruments(id),
    direction TEXT,
    amount NUMERIC,
    amount_minor BIGINT,
    currency CHAR(3),
    event_time DATETIME,
    event_time_confidence TEXT,
    posting_date DATE,
    merchant_raw TEXT,
    merchant_normalized TEXT,
    reference_id TEXT,
    original_amount_minor BIGINT,
    original_currency CHAR(3),
    exchange_rate NUMERIC,
    balance_after_transaction NUMERIC,
    timezone_at_ingestion TEXT,
    fingerprint TEXT,
    extraction_method TEXT,
    confidence_score NUMERIC,
    raw_payload_json JSONB,
    parser_version TEXT,
    emi_total_installments INTEGER,
    emi_installment_number INTEGER,
    emi_original_amount_minor BIGINT,
    is_deleted BOOLEAN DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(source_pipeline, source_record_id)
);
INSERT INTO transaction_observations_new SELECT * FROM transaction_observations;
DROP TABLE transaction_observations;
ALTER TABLE transaction_observations_new RENAME TO transaction_observations;

CREATE INDEX idx_transaction_observations_fingerprint ON transaction_observations(fingerprint);
CREATE INDEX idx_transaction_observations_reference_id ON transaction_observations(reference_id);
