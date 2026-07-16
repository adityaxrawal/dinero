-- TASK-API-007: Document 18 §4.9's `categories` schema (already
-- implemented by TASK-DB-014's migration 20260101000031 rewrite) has no
-- `color`/`icon` columns, but Document 30 TASK-API-007's task text
-- explicitly requires "icon/color customization is allowed" on
-- `categories_update`, with its own named acceptance test
-- (`test_system_category_icon_update_allowed`). Per Aditya's decision
-- resolving this Doc18/Doc30 conflict (2026-07-16, matching how the earlier
-- transactions.notes Doc18/Doc19 gap was resolved): add the columns back
-- via migration rather than dropping the feature.
ALTER TABLE categories ADD COLUMN color TEXT;
ALTER TABLE categories ADD COLUMN icon TEXT;
