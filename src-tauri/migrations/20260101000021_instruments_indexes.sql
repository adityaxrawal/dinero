-- TASK-DB-005: Document 18 §4.2/§6 specify these two indexes; the migrated
-- schema (migration 001) only carried the UNIQUE(type, issuer_name,
-- masked_identifier) constraint. idx_instruments_issuer in particular is
-- called out by name: "since the UI groups the Instruments screen by bank
-- (Document 13 §8.3, Document 14 §8.3), a dedicated index on the grouping
-- key avoids relying solely on the composite unique index for this access
-- pattern."
CREATE INDEX IF NOT EXISTS idx_instruments_type ON instruments(type);
CREATE INDEX IF NOT EXISTS idx_instruments_issuer ON instruments(issuer_name);
