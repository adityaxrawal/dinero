-- TASK-DB-008: Document 30 requires a CHECK constraint on source_pipeline;
-- transaction_observations had none (feedback_log already enforces the
-- same conceptual enum, so this brings the two tables in line). Uses
-- Document 18's authoritative three values (gmail_transaction /
-- statement_pdf / manual), not Document 30's own abbreviated paraphrase
-- ("email/statement/manual") -- these are the exact values already used
-- everywhere else in the codebase (feedback_log's CHECK, this table's own
-- Rust doc comments).
--
-- SQLite has no ALTER TABLE ... ADD CONSTRAINT, so this uses the same
-- create-copy-drop-rename pattern already established in migration 013.
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
    UNIQUE(source_pipeline, source_record_id),
    UNIQUE(fingerprint)
);
INSERT INTO transaction_observations_new SELECT * FROM transaction_observations;
DROP TABLE transaction_observations;
ALTER TABLE transaction_observations_new RENAME TO transaction_observations;
