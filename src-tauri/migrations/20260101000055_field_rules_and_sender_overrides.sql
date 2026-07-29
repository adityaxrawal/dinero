-- Feedback-driven extraction pipeline (design 2026-07-29).
--
-- Replaces pattern_rules/pattern_rule_variants wholesale. The old tables held
-- rules that a human had to read and approve; every rule written from here on
-- is machine-authored and machine-validated, so the schema carries the
-- provenance (authored_by, learned_from) and the audit trail (rule_change_log)
-- that make skipping that approval accountable after the fact.
--
-- No data is migrated across. The old rows are either placeholder regexes that
-- never matched anything (log_user_correction's "learned regex for X") or
-- hint regexes awaiting an approval step that no longer exists; carrying them
-- forward would seed the new system with exactly the rules it exists to
-- replace.

-- Parent: one row per (bank, field, source). The unique constraint is what
-- makes "one rule per bank per field per source" structural rather than
-- convention.
CREATE TABLE field_rules (
    id TEXT PRIMARY KEY,
    bank_name TEXT NOT NULL,
    field_name TEXT NOT NULL,
    source_type TEXT NOT NULL CHECK(source_type IN ('email', 'statement_pdf')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(bank_name, field_name, source_type)
);

-- Child: one row per learned (template shape -> rule payload) pair.
-- rule_payload_json is either {"regex": "...", "capture_group": 1} for a span
-- field or {"override_value": "..."} for direction/currency, which are
-- template-level literals with no span to anchor on.
CREATE TABLE field_rule_variants (
    id TEXT PRIMARY KEY,
    field_rule_id TEXT NOT NULL REFERENCES field_rules(id) ON DELETE CASCADE,
    template_hash TEXT NOT NULL,
    rule_payload_json JSONB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'active', 'trusted', 'inactive', 'flagged')),
    success_count INTEGER NOT NULL DEFAULT 0,
    failure_count INTEGER NOT NULL DEFAULT 0,
    confidence NUMERIC NOT NULL DEFAULT 0.0,
    authored_by TEXT NOT NULL CHECK(authored_by IN ('deterministic', 'llm')),
    learned_from TEXT NOT NULL CHECK(learned_from IN ('user_edit', 'drift_llm', 'batch_cleanup')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Partial, not blanket: an inactive/flagged variant (decayed or reverted) must
-- never block a fresh attempt at the same template. Carries forward the exact
-- guarantee migrations 48/49 established for pattern_rule_variants.
CREATE UNIQUE INDEX idx_field_rule_variants_live
    ON field_rule_variants(field_rule_id, template_hash)
    WHERE status IN ('pending', 'active', 'trusted');

-- Extraction's hot path: every live variant for one bank+source.
CREATE INDEX idx_field_rule_variants_lookup
    ON field_rule_variants(field_rule_id, status);

-- Full history of what was written, why, and what it replaced. A 'rejected'
-- row has no variant (nothing was written), so field_rule_variant_id is
-- nullable and deliberately carries no FK -- this is history, not live state,
-- and must survive the variant being deleted.
CREATE TABLE rule_change_log (
    id TEXT PRIMARY KEY,
    field_rule_variant_id TEXT,
    action TEXT NOT NULL CHECK(action IN ('created', 'activated', 'reverted', 'rejected')),
    old_payload_json JSONB,
    new_payload_json JSONB,
    triggering_feedback_log_id TEXT,
    reason TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_rule_change_log_variant ON rule_change_log(field_rule_variant_id);
CREATE INDEX idx_rule_change_log_created ON rule_change_log(created_at DESC);

-- Gate 1's relabel layer: "this verified sender is actually <bank>". Never
-- grants verification -- see ingestion/message_processor.rs. Domain-scoped and
-- UNIQUE on domain, because the blast radius of a domain-wide misroute is the
-- highest in this design and a per-address variant would multiply the ways to
-- get it wrong.
CREATE TABLE sender_bank_overrides (
    id TEXT PRIMARY KEY,
    domain TEXT NOT NULL UNIQUE,
    bank_name TEXT NOT NULL,
    display_name TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'inactive')),
    triggering_feedback_log_id TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_sender_bank_overrides_active
    ON sender_bank_overrides(domain) WHERE status = 'active';

-- The trigger was created on the original flat pattern_rules in migration 6 and
-- followed the ALTER TABLE RENAME in migration 48, so it now fires on
-- pattern_rule_variants under its original name. Dropped explicitly rather than
-- relying on DROP TABLE's implicit cleanup.
DROP TRIGGER IF EXISTS updated_at_trigger_pattern_rules;

DROP TABLE IF EXISTS pattern_rule_variants;
DROP TABLE IF EXISTS pattern_rules;
