-- TASK-DB-006: Document 18 §6 (Indexing Strategy) specifies four indexes
-- for the canonical `transactions` table ("Dedup, lookup, filter,
-- analytics") -- none existed anywhere in the migrated schema. This
-- matters concretely, not just per the doc: `transactions.rs`'s own
-- `find_exact_match`/`find_candidates_within_window` (the reconciliation
-- engine's dedup lookups, Document 12 §8.2a) query exactly on
-- `(instrument_id, amount_minor, currency, direction, ...)` -- without
-- these indexes every dedup candidate lookup is a full table scan, at a
-- table Document 18 §11.1 sizes up to a 500,000-row soft limit.
CREATE INDEX IF NOT EXISTS idx_transactions_instrument_event
    ON transactions(instrument_id, amount_minor, currency, direction, best_event_time);
CREATE INDEX IF NOT EXISTS idx_transactions_instrument_posting
    ON transactions(instrument_id, amount_minor, currency, direction, best_posting_date);
CREATE INDEX IF NOT EXISTS idx_transactions_reference_id ON transactions(reference_id);
CREATE INDEX IF NOT EXISTS idx_transactions_merchant_normalized ON transactions(merchant_normalized_name);
