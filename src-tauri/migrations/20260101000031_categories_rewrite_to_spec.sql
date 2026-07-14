-- TASK-DB-014: `categories` (migration 002) predates Document 18 SS4.9 and
-- deviates: it has `color`/`icon`/`is_system` (boolean) and `updated_at`,
-- none of which Document 18 specifies, while missing `source_type` (a
-- 3-value `system`/`user`/`mcc_mapped` enum -- a real distinction the
-- boolean `is_system` can't express: MCC-derived categories are neither
-- hand-curated system categories nor user-created ones), `mcc_code`, and
-- `monthly_budget_minor` (per-category spend limits, referenced by Document
-- 30's spending-limits area but with nowhere to live under the old schema).
-- Checked every real usage first: color/icon/CategoriesRow are only
-- referenced from db/categories.rs and its own tests -- no frontend
-- consumer exists yet (Area 9 UI not reached) -- so this is a clean rewrite.
-- `categories` is self-referencing (parent_id -> categories.id), so a plain
-- create-new/copy/drop-old/rename would leave `categories_new`'s freshly
-- inserted rows pointing at the *old* table while it's still the live FK
-- target -- DROP TABLE's implicit delete then orphans them and SQLite
-- raises FOREIGN KEY constraint failed. Copying through a plain temp table
-- with no FK constraint of its own sidesteps this: nothing is ever a live
-- FK child of the table being dropped.
CREATE TEMP TABLE categories_backup AS SELECT id, parent_id, name, is_system, is_deleted, created_at FROM categories;
DROP TABLE categories;
CREATE TABLE categories (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES categories(id),
    name TEXT NOT NULL UNIQUE,
    source_type TEXT NOT NULL DEFAULT 'system' CHECK(source_type IN ('system', 'user', 'mcc_mapped')),
    mcc_code TEXT,
    monthly_budget_minor INTEGER,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
-- Root categories (parent_id IS NULL) first, then children, so no row's
-- self-referencing parent_id is inserted ahead of the parent it points to.
INSERT INTO categories (id, parent_id, name, source_type, is_deleted, created_at)
SELECT id, parent_id, name, CASE WHEN is_system = 1 THEN 'system' ELSE 'user' END, is_deleted, created_at
FROM categories_backup
ORDER BY CASE WHEN parent_id IS NULL THEN 0 ELSE 1 END;
DROP TABLE categories_backup;

-- The updated_at_trigger_categories trigger (migration 005) references a
-- column that no longer exists on this table; Document 18 doesn't list
-- updated_at for categories, so the trigger is dropped rather than the
-- column re-added.
DROP TRIGGER IF EXISTS updated_at_trigger_categories;
