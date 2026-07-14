CREATE TABLE IF NOT EXISTS processing_checkpoints (
    id TEXT PRIMARY KEY,
    job_type TEXT NOT NULL,
    job_key TEXT NOT NULL,
    checkpoint_state_json JSONB NOT NULL,
    last_processed_token TEXT,
    status TEXT NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(job_type, job_key)
);

CREATE TABLE IF NOT EXISTS unprocessed_statements (
    id TEXT PRIMARY KEY,
    statement_source_json JSONB NOT NULL,
    failure_type TEXT NOT NULL,
    failure_reason TEXT NOT NULL,
    status TEXT NOT NULL,
    resolved_statement_id TEXT REFERENCES statements(id),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS unassigned_transactions (
    id TEXT PRIMARY KEY,
    observation_id TEXT NOT NULL REFERENCES transaction_observations(id),
    reason TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_processing_checkpoints
AFTER UPDATE ON processing_checkpoints FOR EACH ROW
BEGIN UPDATE processing_checkpoints SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_unprocessed_statements
AFTER UPDATE ON unprocessed_statements FOR EACH ROW
BEGIN UPDATE unprocessed_statements SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;
