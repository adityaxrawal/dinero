-- 0013: Foreign Key Delete Semantics

PRAGMA foreign_keys=off;

-- statement_entries (statement_id CASCADE)
CREATE TABLE statement_entries_new (
    id TEXT PRIMARY KEY,
    statement_id TEXT REFERENCES statements(id) ON DELETE CASCADE,
    row_index INTEGER,
    transaction_date DATE,
    posting_date DATE,
    description_raw TEXT,
    merchant_raw TEXT,
    merchant_normalized TEXT,
    amount NUMERIC,
    amount_minor BIGINT,
    currency CHAR(3),
    direction TEXT,
    reference_id TEXT,
    location TEXT,
    raw_row_json JSONB,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO statement_entries_new SELECT * FROM statement_entries;
DROP TABLE statement_entries;
ALTER TABLE statement_entries_new RENAME TO statement_entries;

-- transaction_tags (transaction_id CASCADE)
CREATE TABLE transaction_tags_new (
    transaction_id TEXT REFERENCES transactions(id) ON DELETE CASCADE,
    tag_id TEXT REFERENCES tags(id) ON DELETE RESTRICT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (transaction_id, tag_id)
);
INSERT INTO transaction_tags_new SELECT * FROM transaction_tags;
DROP TABLE transaction_tags;
ALTER TABLE transaction_tags_new RENAME TO transaction_tags;

-- match_decisions (observation_id RESTRICT, matched_transaction_id RESTRICT)
CREATE TABLE match_decisions_new (
    id TEXT PRIMARY KEY,
    observation_id TEXT REFERENCES transaction_observations(id) ON DELETE RESTRICT,
    matched_transaction_id TEXT REFERENCES transactions(id) ON DELETE RESTRICT,
    decision TEXT,
    score NUMERIC,
    rules_triggered_json JSONB,
    review_status TEXT,
    reviewed_by TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO match_decisions_new SELECT * FROM match_decisions;
DROP TABLE match_decisions;
ALTER TABLE match_decisions_new RENAME TO match_decisions;

-- Recreate immutable trigger for match_decisions since table was dropped
CREATE TRIGGER IF NOT EXISTS immutable_match_decisions
BEFORE UPDATE ON match_decisions
BEGIN
    SELECT RAISE(ABORT, 'match_decisions is immutable');
END;

PRAGMA foreign_keys=on;
