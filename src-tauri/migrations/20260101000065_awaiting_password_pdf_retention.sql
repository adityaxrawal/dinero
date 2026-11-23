-- Review L5: PDFs awaiting a password had no retention date and were kept
-- indefinitely. They now expire 7 days after creation; existing ones get 7
-- days from this migration.
UPDATE unprocessed_statements
SET pdf_retained_until = datetime('now', '+7 days')
WHERE status = 'awaiting_password' AND pdf_retained_until IS NULL;
