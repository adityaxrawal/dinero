-- Doc-30-style optimization #5: Gate 2's "Noise"/"Unknown" classification is
-- a heuristic, not a certainty. Hard-discarding it (as every other rejected
-- ContentClass still does) means a Gate-2 misfire on a real transaction is
-- unrecoverable. This table is a 30-day-expiring parking lot for exactly
-- those two classes so a user's "why is this transaction missing" report can
-- be cross-checked against what Gate 2 almost-but-not-quite threw away.
CREATE TABLE IF NOT EXISTS ignored_messages (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    bank_name TEXT,
    reason TEXT NOT NULL,
    subject TEXT,
    snippet TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ignored_messages_expires_at ON ignored_messages(expires_at);
CREATE INDEX IF NOT EXISTS idx_ignored_messages_message_id ON ignored_messages(message_id);
