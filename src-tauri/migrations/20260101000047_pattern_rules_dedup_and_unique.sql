-- Collapses existing duplicate pattern_rules rows down to one per
-- (bank_name, template_hash, field_name) -- `BankTemplateLayer` and the
-- LLM drift-synthesis path (extraction/ladder.rs) inserted a brand new row
-- on every matching email with no existence check, so the same triple
-- could accumulate unboundedly. Keeps the most-trusted survivor: highest
-- status rank, then most successes, then the oldest row.
DELETE FROM pattern_rules
WHERE rowid NOT IN (
    SELECT (
        SELECT p2.rowid FROM pattern_rules p2
        WHERE p2.bank_name = p1.bank_name
          AND p2.template_hash = p1.template_hash
          AND p2.field_name = p1.field_name
        ORDER BY
            CASE p2.status
                WHEN 'trusted' THEN 0
                WHEN 'active' THEN 1
                WHEN 'pending' THEN 2
                WHEN 'flagged' THEN 3
                WHEN 'inactive' THEN 4
                ELSE 5
            END ASC,
            p2.success_count DESC,
            p2.created_at ASC,
            p2.rowid ASC
        LIMIT 1
    )
    FROM pattern_rules p1
);

-- Upgrade the existing lookup index into a uniqueness guarantee so this
-- invariant can't silently regress -- same columns, same name, no second
-- redundant index.
DROP INDEX IF EXISTS idx_pattern_rules_lookup;
CREATE UNIQUE INDEX idx_pattern_rules_lookup ON pattern_rules(bank_name, template_hash, field_name);
