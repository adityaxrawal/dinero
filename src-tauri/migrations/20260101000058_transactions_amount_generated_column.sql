-- audit_05 #4: `transactions` carried both `amount` (float, major units) and
-- `amount_minor` (integer, minor units), kept in sync by hand on every write.
-- Two independent columns holding the same value can silently diverge, and any
-- code path that reached for `amount` in a financial calculation would inherit
-- f64 rounding error -- `f64` cannot exactly represent most monetary values.
--
-- `amount` is not dropped outright: 18 read sites depend on it, including the
-- search query's `CAST(t.amount AS TEXT) LIKE ?` merchant/amount text match
-- (db/transactions.rs) and the `COALESCE(t.amount, o.amount, 0)` analytics
-- projection (commands/data.rs). Making it a VIRTUAL generated column keeps
-- every one of those reads working while making divergence structurally
-- impossible: `amount_minor` becomes the single source of truth, and SQLite
-- now rejects any write to `amount` outright rather than trusting callers to
-- keep the pair consistent.
--
-- Verified before writing this as two ALTERs rather than a full table rebuild:
-- no index, trigger, view, or FTS5 definition anywhere in migrations/ or
-- src/ references `transactions.amount` (the transactions_fts triggers index
-- only merchant_display_name / merchant_normalized_name / reference_id /
-- location), so DROP COLUMN has nothing to invalidate. ADD COLUMN permits
-- GENERATED ... VIRTUAL; STORED would require the rebuild we are avoiding.
--
-- Every production writer already computed exactly this expression
-- (`obs.amount_minor as f64 / 100.0` in reconciliation/canonical.rs,
-- commands/mod.rs, extraction/ladder.rs), so no stored value changes meaning.
-- The column moves to the end of the table; all row readers are name-based
-- (`row.get("amount")`), so ordinal position is not depended upon.

ALTER TABLE transactions DROP COLUMN amount;

ALTER TABLE transactions
    ADD COLUMN amount REAL GENERATED ALWAYS AS (amount_minor / 100.0) VIRTUAL;
