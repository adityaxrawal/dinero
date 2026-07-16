-- TASK-API-007: `categories_delete` (Doc 30) reassigns a deleted user
-- category's linked transactions to "Others" -- no such category existed
-- anywhere in the seed data (migration 20260101000002's 11 seeded
-- categories). Seeded as source_type = 'system' so it can never itself be
-- renamed or deleted (categories.rs's existing update()/soft_delete()
-- guards already enforce this for any source_type = 'system' row), which
-- avoids the categories_delete target ever disappearing out from under it.
INSERT INTO categories (id, name, source_type, is_deleted)
VALUES ('cat_others', 'Others', 'system', 0)
ON CONFLICT(name) DO NOTHING;
