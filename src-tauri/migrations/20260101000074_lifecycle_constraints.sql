-- Audit C-3 (owner decision: unique open review + closed lifecycles only).
-- Runs with foreign keys off (C-5); a violating existing value fails the
-- migration, and the pre-migration backup is restored.

-- One observation, at most one open review entry: keep the oldest.
DELETE FROM unassigned_transactions
WHERE status = 'open'
  AND id NOT IN (
      SELECT id FROM (
          SELECT id, ROW_NUMBER() OVER (
              PARTITION BY observation_id ORDER BY created_at, id
          ) AS n
          FROM unassigned_transactions
          WHERE status = 'open'
      ) WHERE n = 1
  );

CREATE TABLE unassigned_transactions_new (
    id TEXT PRIMARY KEY,
    observation_id TEXT NOT NULL REFERENCES transaction_observations(id),
    reason TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'resolved', 'ignored')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO unassigned_transactions_new (id, observation_id, reason, status, created_at)
    SELECT id, observation_id, reason, status, created_at FROM unassigned_transactions;
DROP TABLE unassigned_transactions;
ALTER TABLE unassigned_transactions_new RENAME TO unassigned_transactions;
CREATE INDEX idx_unassigned_transactions_status ON unassigned_transactions(status);
CREATE INDEX idx_unassigned_observation ON unassigned_transactions(observation_id);
CREATE UNIQUE INDEX idx_unassigned_one_open_per_observation
    ON unassigned_transactions(observation_id) WHERE status = 'open';

CREATE TABLE statement_drafts_new (
    id TEXT PRIMARY KEY,
    origin TEXT NOT NULL,
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
    status TEXT NOT NULL DEFAULT 'pending_review'
        CHECK (status IN ('pending_review', 'committing', 'committed', 'discarded')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    missing_pages TEXT
);
INSERT INTO statement_drafts_new
    SELECT id, origin, file_hash, instrument_id, issuer_name, masked_identifier, instrument_type,
           billing_period_start, billing_period_end, due_date, statement_date, current_balance,
           minimum_due, rows_json, status, created_at, updated_at, missing_pages
    FROM statement_drafts;
DROP TABLE statement_drafts;
ALTER TABLE statement_drafts_new RENAME TO statement_drafts;
CREATE INDEX idx_statement_drafts_status ON statement_drafts(status);
CREATE TRIGGER updated_at_trigger_statement_drafts
AFTER UPDATE ON statement_drafts FOR EACH ROW
BEGIN UPDATE statement_drafts SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;
