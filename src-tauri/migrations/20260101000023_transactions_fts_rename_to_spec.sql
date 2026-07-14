-- TASK-DB-007: migration 011 replaced the original transactions_fts with a
-- table named `transactionsfts` (no underscore) indexing shadow
-- GENERATED ALWAYS AS VIRTUAL columns (merchantdisplayname,
-- merchantnormalizedname, referenceid) instead of the real columns.
-- Verified empirically that Document 18 §4.3a's simpler DDL -- an FTS5
-- external-content table indexing the real column names directly -- works
-- with no issue; the shadow-column workaround was solving nothing.
-- Document 15 Core Principle 6 / Step C ("wrong table") calls for matching
-- the doc exactly rather than accommodating an unnecessary deviation.

DROP TRIGGER IF EXISTS transactions_ai;
DROP TRIGGER IF EXISTS transactions_ad;
DROP TRIGGER IF EXISTS transactions_au;
DROP TABLE IF EXISTS transactionsfts;

ALTER TABLE transactions DROP COLUMN merchantdisplayname;
ALTER TABLE transactions DROP COLUMN merchantnormalizedname;
ALTER TABLE transactions DROP COLUMN referenceid;

CREATE VIRTUAL TABLE transactions_fts USING fts5(
    id UNINDEXED,
    merchant_display_name,
    merchant_normalized_name,
    reference_id,
    location,
    content='transactions',
    content_rowid='rowid'
);

CREATE TRIGGER transactions_ai AFTER INSERT ON transactions BEGIN
    INSERT INTO transactions_fts(rowid, id, merchant_display_name, merchant_normalized_name, reference_id, location)
    VALUES (new.rowid, new.id, new.merchant_display_name, new.merchant_normalized_name, new.reference_id, new.location);
END;

CREATE TRIGGER transactions_ad AFTER DELETE ON transactions BEGIN
    INSERT INTO transactions_fts(transactions_fts, rowid, id, merchant_display_name, merchant_normalized_name, reference_id, location)
    VALUES ('delete', old.rowid, old.id, old.merchant_display_name, old.merchant_normalized_name, old.reference_id, old.location);
END;

CREATE TRIGGER transactions_au AFTER UPDATE ON transactions BEGIN
    INSERT INTO transactions_fts(transactions_fts, rowid, id, merchant_display_name, merchant_normalized_name, reference_id, location)
    VALUES ('delete', old.rowid, old.id, old.merchant_display_name, old.merchant_normalized_name, old.reference_id, old.location);
    INSERT INTO transactions_fts(rowid, id, merchant_display_name, merchant_normalized_name, reference_id, location)
    VALUES (new.rowid, new.id, new.merchant_display_name, new.merchant_normalized_name, new.reference_id, new.location);
END;

-- Deterministic backfill
INSERT INTO transactions_fts(rowid, id, merchant_display_name, merchant_normalized_name, reference_id, location)
SELECT rowid, id, merchant_display_name, merchant_normalized_name, reference_id, location
FROM transactions WHERE is_deleted = 0;
