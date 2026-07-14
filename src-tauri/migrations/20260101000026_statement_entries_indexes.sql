-- TASK-DB-010: Document 18 §6 specifies two indexes for statement_entries
-- ("Row ordering and reporting") -- neither existed. Using Document 18's
-- exact two-index split, (statement_id, row_index) and (merchant_normalized)
-- separately, rather than Document 30's own combined three-column version.
CREATE INDEX IF NOT EXISTS idx_statement_entries_statement_row
    ON statement_entries(statement_id, row_index);
CREATE INDEX IF NOT EXISTS idx_statement_entries_merchant_normalized
    ON statement_entries(merchant_normalized);
