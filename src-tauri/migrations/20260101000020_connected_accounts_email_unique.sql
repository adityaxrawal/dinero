-- TASK-DB-004: Document 30 specifies `email_address (unique)` on
-- connected_accounts -- the initial schema (migration 001) omitted the
-- constraint entirely, which would let the same Gmail account be
-- "connected" twice, wasting a slot against the 10-account cap
-- (Document 03 §8.2) and creating ambiguous duplicate-account state.
-- SQLite treats multiple NULLs as distinct under a UNIQUE index, so this
-- does not conflict with any row that has not yet resolved its email.
CREATE UNIQUE INDEX IF NOT EXISTS idx_connected_accounts_email_unique
    ON connected_accounts(email_address);
