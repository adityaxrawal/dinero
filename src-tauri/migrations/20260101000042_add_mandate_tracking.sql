-- Explicit mandate tracking (dinero-docs/design-archive/specs/2026-07-18-mandate-tracking-design.md).
-- `source` distinguishes rows written by recurring_detector.rs's statistical
-- inference (existing rows backfill 'inferred', the default) from rows
-- written directly from an explicit bank mandate-registration email
-- ('explicit'). `external_mandate_id` is the bank's own mandate reference
-- (e.g. SBI Card's "SiHub ID"), nullable since not every bank prints one.
ALTER TABLE recurring_payments ADD COLUMN source TEXT NOT NULL DEFAULT 'inferred';
ALTER TABLE recurring_payments ADD COLUMN external_mandate_id TEXT;

-- Mirrors unprocessed_statements' blocking-on-user-input shape (Doc 18
-- §4.16-4.21): a cancellation email that couldn't be matched to exactly one
-- active recurring_payments row (by external_mandate_id, else by
-- merchant+instrument) is never guessed at -- it's logged here instead.
CREATE TABLE IF NOT EXISTS unresolved_mandate_cancellations (
    id TEXT PRIMARY KEY,
    raw_signal TEXT NOT NULL,
    candidate_ids TEXT,
    status TEXT NOT NULL DEFAULT 'unresolved',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    resolved_at DATETIME
);
