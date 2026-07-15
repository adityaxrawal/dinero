-- Doc 30 TASK-API-003: Document 19 §8.3 lists `notes` as an editable
-- `transactions_update` field, but Document 18 §4.3's `transactions`
-- schema never had a `notes` column -- a genuine documentation gap between
-- the schema-authoritative and IPC-authoritative documents, flagged and
-- resolved by Aditya's decision (2026-07-16): add the column so the IPC
-- contract Document 19 already promises actually works end-to-end.
ALTER TABLE transactions ADD COLUMN notes TEXT;
