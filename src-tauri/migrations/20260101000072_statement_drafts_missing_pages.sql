-- Review P3: pages that neither the text layer nor OCR could read used to be
-- dropped silently, so a draft could hold only some of a statement's rows.
-- JSON array of 1-based page numbers; NULL when every page was read.
ALTER TABLE statement_drafts ADD COLUMN missing_pages TEXT;
