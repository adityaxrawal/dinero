-- Doc 2026-07-28 dev-scan-log-issues: an exhausted-retry Gmail fetch
-- failure was previously only logged (tracing::error!), never persisted
-- anywhere queryable -- a transaction could vanish from a scan with
-- nothing user-visible beyond an incremented `errors` counter. This table
-- makes those failures durable and queryable so a "why is this
-- transaction missing" report can be cross-checked against what actually
-- failed to fetch.
CREATE TABLE IF NOT EXISTS scan_failed_messages (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    msg_id TEXT NOT NULL,
    error TEXT NOT NULL,
    failed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_scan_failed_messages_account_id ON scan_failed_messages(account_id);
