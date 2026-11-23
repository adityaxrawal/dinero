-- Review P2: one live variant per (rule, template) forced a new rule to
-- overwrite the working one in place, so a drift-LLM rule with no evidence
-- could demote a trusted rule. Now one variant serves (active/trusted) and at
-- most one pending challenger is evaluated beside it in shadow.
DROP INDEX IF EXISTS idx_field_rule_variants_live;

CREATE UNIQUE INDEX idx_field_rule_variants_serving
    ON field_rule_variants(field_rule_id, template_hash)
    WHERE status IN ('active', 'trusted');

CREATE UNIQUE INDEX idx_field_rule_variants_challenger
    ON field_rule_variants(field_rule_id, template_hash)
    WHERE status = 'pending';
