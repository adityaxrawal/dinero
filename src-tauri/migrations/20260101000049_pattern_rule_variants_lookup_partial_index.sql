-- Migration 48's uniqueness guarantee on (pattern_rule_id, template_hash) is
-- supposed to be partial -- only 'pending'/'active'/'trusted' variants should
-- collide, so a decayed/rejected variant never blocks a fresh relearning
-- attempt for the same template. A dev build of 48 got applied without that
-- predicate before it was added to the migration file, so this re-asserts it
-- unconditionally; harmless if the index already has it.
DROP INDEX IF EXISTS idx_pattern_rule_variants_lookup;
CREATE UNIQUE INDEX idx_pattern_rule_variants_lookup
    ON pattern_rule_variants(pattern_rule_id, template_hash)
    WHERE status IN ('pending', 'active', 'trusted');
