-- Durable mirror of the in-memory Layer 6 mpsc channel. A message that
-- clears Gate 2 but fails Gate 3 (or all regex layers) on an LLM-eligible
-- machine is enqueued for background LLM extraction -- previously that
-- enqueue lived only in the channel's in-flight buffer, so any app restart
-- while a job was still sitting there (not yet dequeued) silently and
-- permanently dropped it, with no error and no way to tell from the UI.
-- `id` is the `unassigned_transactions.id` this job resolves -- inserted
-- alongside the channel send, deleted the moment the job is dequeued for
-- processing (see `queues::process_layer6_job`), and replayed into the
-- channel at startup for anything still present.
CREATE TABLE layer6_pending_jobs (
    id TEXT PRIMARY KEY,
    observation_id TEXT NOT NULL,
    bank_name TEXT NOT NULL,
    body_text TEXT NOT NULL,
    internal_date_seconds INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
