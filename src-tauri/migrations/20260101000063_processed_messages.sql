-- Review M2: dedupe only knew about ignored messages, observations and
-- statements, so messages rejected with only an audit row, and mandate
-- events, were refetched (two Gmail calls each) and re-audited on every
-- rescan. Final decisions for a message are recorded here and dedupe checks it.
CREATE TABLE processed_messages (
    message_id TEXT PRIMARY KEY,
    decision TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
