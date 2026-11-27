-- current-issues #4: lines on readable pages that look like transactions (a
-- date and an amount) but did not become rows, so draft review can show them
-- and let the user add them. JSON array; NULL when there are none.
ALTER TABLE statement_drafts ADD COLUMN missed_lines TEXT;
