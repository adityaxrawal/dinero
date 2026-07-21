CREATE TABLE IF NOT EXISTS statement_drafts (
    id TEXT PRIMARY KEY,
    origin TEXT NOT NULL,                -- 'password_unlock' | 'manual_upload' | 'email_scan'
    file_hash TEXT NOT NULL,
    instrument_id TEXT REFERENCES instruments(id),
    issuer_name TEXT,
    masked_identifier TEXT,
    instrument_type TEXT,
    billing_period_start DATE,
    billing_period_end DATE,
    due_date DATE,
    statement_date DATE,
    current_balance BIGINT,
    minimum_due BIGINT,
    rows_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending_review',  -- 'pending_review' | 'committed' | 'discarded'
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_statement_drafts_status ON statement_drafts(status);

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_statement_drafts
AFTER UPDATE ON statement_drafts FOR EACH ROW
BEGIN UPDATE statement_drafts SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;
