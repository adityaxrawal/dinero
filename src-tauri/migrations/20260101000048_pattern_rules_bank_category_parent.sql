-- Splits the flat pattern_rules table into a parent (one row per
-- bank_name + field_name -- "one pattern rule per bank per category") and
-- a child pattern_rule_variants table (one row per learned regex/template
-- combination). A new email template format for a bank+category already
-- covered now merges in as an additional variant under the existing
-- parent instead of creating a second top-level rule or overwriting the
-- working regex.

-- Step 1: move the existing flat table out of the way; it becomes the
-- variant table.
ALTER TABLE pattern_rules RENAME TO pattern_rule_variants;
DROP INDEX IF EXISTS idx_pattern_rules_lookup;

-- Step 2: create the new slim parent table.
CREATE TABLE pattern_rules (
    id TEXT PRIMARY KEY,
    bank_name TEXT NOT NULL,
    field_name TEXT NOT NULL,
    created_at DATETIME,
    updated_at DATETIME,
    UNIQUE(bank_name, field_name)
);

-- Step 3: backfill one parent per distinct (bank_name, field_name) already
-- present among the renamed variants (already collapsed to at most one
-- row per full (bank, hash, field) triple by migration 47's dedup pass).
INSERT INTO pattern_rules (id, bank_name, field_name, created_at, updated_at)
SELECT
    lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-' ||
          hex(randomblob(2)) || '-' || hex(randomblob(2)) || '-' ||
          hex(randomblob(6))),
    bank_name,
    field_name,
    MIN(created_at),
    MAX(updated_at)
FROM pattern_rule_variants
GROUP BY bank_name, field_name;

-- Step 4: point every variant at its new parent.
ALTER TABLE pattern_rule_variants ADD COLUMN pattern_rule_id TEXT REFERENCES pattern_rules(id);

UPDATE pattern_rule_variants
SET pattern_rule_id = (
    SELECT id FROM pattern_rules p
    WHERE p.bank_name = pattern_rule_variants.bank_name
      AND p.field_name = pattern_rule_variants.field_name
);

-- Step 5: bank_name/field_name now live on the parent only -- drop them
-- from the variant table so there's one source of truth.
ALTER TABLE pattern_rule_variants DROP COLUMN bank_name;
ALTER TABLE pattern_rule_variants DROP COLUMN field_name;

-- Step 6: a given template can only produce one LIVE variant per parent --
-- partial, not a blanket unique index: an inactive/flagged variant (decayed
-- or rejected) must not block a fresh pending attempt for that same
-- template, matching candidate_exists()'s own status filter.
CREATE UNIQUE INDEX idx_pattern_rule_variants_lookup
    ON pattern_rule_variants(pattern_rule_id, template_hash)
    WHERE status IN ('pending', 'active', 'trusted');
