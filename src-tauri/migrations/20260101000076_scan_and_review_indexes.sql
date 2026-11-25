-- Audit N-C3 (owner decision): two lookups that still scanned or sorted.
-- Every scanned email asks whether a statement already came from it.
CREATE INDEX IF NOT EXISTS idx_statements_source_message_id
    ON statements(source_message_id);
-- The review queue lists open entries newest first.
CREATE INDEX IF NOT EXISTS idx_unassigned_status_created
    ON unassigned_transactions(status, created_at, id);
