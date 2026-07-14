CREATE VIRTUAL TABLE IF NOT EXISTS transactions_fts USING fts5(
    transaction_id UNINDEXED,
    merchant_display_name,
    reference_id,
    location,
    transaction_subtype
);

CREATE TRIGGER IF NOT EXISTS tx_after_insert AFTER INSERT ON transactions
BEGIN
    INSERT INTO transactions_fts (rowid, transaction_id, merchant_display_name, reference_id, location, transaction_subtype)
    VALUES (new.rowid, new.id, new.merchant_display_name, new.reference_id, new.location, new.transaction_subtype);
END;

CREATE TRIGGER IF NOT EXISTS tx_after_update AFTER UPDATE ON transactions
BEGIN
    DELETE FROM transactions_fts WHERE rowid = old.rowid;
    INSERT INTO transactions_fts (rowid, transaction_id, merchant_display_name, reference_id, location, transaction_subtype)
    VALUES (new.rowid, new.id, new.merchant_display_name, new.reference_id, new.location, new.transaction_subtype);
END;

CREATE TRIGGER IF NOT EXISTS tx_after_delete AFTER DELETE ON transactions
BEGIN
    DELETE FROM transactions_fts WHERE rowid = old.rowid;
END;
