CREATE TABLE IF NOT EXISTS pdf_passwords (
    id TEXT PRIMARY KEY,
    instrument_id TEXT NOT NULL REFERENCES instruments(id),
    password_ciphertext TEXT NOT NULL,
    success_count INTEGER NOT NULL DEFAULT 0,
    last_used_at DATETIME,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS pattern_rules (
    id TEXT PRIMARY KEY,
    bank_name TEXT NOT NULL,
    template_hash TEXT NOT NULL,
    field_name TEXT NOT NULL,
    rule_payload_json JSONB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'active', 'trusted', 'inactive', 'flagged')),
    success_count INTEGER NOT NULL DEFAULT 0,
    failure_count INTEGER NOT NULL DEFAULT 0,
    confidence NUMERIC NOT NULL DEFAULT 0.0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS feedback_log (
    id TEXT PRIMARY KEY,
    transaction_id TEXT NOT NULL REFERENCES transactions(id),
    observation_id TEXT REFERENCES transaction_observations(id),
    source_pipeline TEXT CHECK(source_pipeline IN ('gmail_transaction', 'statement_pdf', 'manual')),
    field_name TEXT NOT NULL,
    old_value TEXT,
    new_value TEXT NOT NULL,
    source_context_json JSONB NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_pdf_passwords
AFTER UPDATE ON pdf_passwords FOR EACH ROW
BEGIN UPDATE pdf_passwords SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_pattern_rules
AFTER UPDATE ON pattern_rules FOR EACH ROW
BEGIN UPDATE pattern_rules SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;
