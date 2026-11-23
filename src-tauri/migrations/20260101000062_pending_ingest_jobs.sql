-- Review H4: transaction and mandate jobs lived only in in-memory channels,
-- while the Gmail cursor advanced as soon as they were queued, so a quit or a
-- worker DB failure lost the email for good. Each job is now written here
-- before it is queued, deleted once the worker has finished it, and replayed
-- at the next launch otherwise (the same pattern as layer6_pending_jobs).
-- id = kind || ':' || source_record_id, so re-enqueuing the same message is
-- idempotent.
CREATE TABLE pending_ingest_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('transaction', 'mandate')),
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
