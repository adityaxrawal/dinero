-- Follow-up to review H6: a statement recovered after a crash lost its
-- original filename (it showed as "statement.pdf"). The name is now stored
-- when the statement is queued.
ALTER TABLE statements ADD COLUMN source_filename TEXT;
