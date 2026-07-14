DROP TRIGGER IF EXISTS tx_after_delete;
DROP TRIGGER IF EXISTS tx_after_update;
DROP TRIGGER IF EXISTS tx_after_insert;
DROP TABLE IF EXISTS transactions_fts;

ALTER TABLE transactions ADD COLUMN merchantdisplayname TEXT GENERATED ALWAYS AS (merchant_display_name) VIRTUAL;
ALTER TABLE transactions ADD COLUMN merchantnormalizedname TEXT GENERATED ALWAYS AS (merchant_normalized_name) VIRTUAL;
ALTER TABLE transactions ADD COLUMN referenceid TEXT GENERATED ALWAYS AS (reference_id) VIRTUAL;

CREATE VIRTUAL TABLE transactionsfts USING fts5 (
    id UNINDEXED,
    merchantdisplayname,
    merchantnormalizedname,
    referenceid,
    location,
    content='transactions',
    content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS transactions_ai AFTER INSERT ON transactions
BEGIN
    INSERT INTO transactionsfts (rowid, id, merchantdisplayname, merchantnormalizedname, referenceid, location)
    VALUES (new.rowid, new.id, new.merchant_display_name, new.merchant_normalized_name, new.reference_id, new.location);
END;

CREATE TRIGGER IF NOT EXISTS transactions_ad AFTER DELETE ON transactions
BEGIN
    INSERT INTO transactionsfts (transactionsfts, rowid, id, merchantdisplayname, merchantnormalizedname, referenceid, location)
    VALUES ('delete', old.rowid, old.id, old.merchant_display_name, old.merchant_normalized_name, old.reference_id, old.location);
END;

CREATE TRIGGER IF NOT EXISTS transactions_au AFTER UPDATE ON transactions
BEGIN
    INSERT INTO transactionsfts (transactionsfts, rowid, id, merchantdisplayname, merchantnormalizedname, referenceid, location)
    VALUES ('delete', old.rowid, old.id, old.merchant_display_name, old.merchant_normalized_name, old.reference_id, old.location);
    INSERT INTO transactionsfts (rowid, id, merchantdisplayname, merchantnormalizedname, referenceid, location)
    VALUES (new.rowid, new.id, new.merchant_display_name, new.merchant_normalized_name, new.reference_id, new.location);
END;

-- Deterministic Backfill
INSERT INTO transactionsfts (rowid, id, merchantdisplayname, merchantnormalizedname, referenceid, location)
SELECT rowid, id, merchant_display_name, merchant_normalized_name, reference_id, location
FROM transactions WHERE is_deleted = 0;
