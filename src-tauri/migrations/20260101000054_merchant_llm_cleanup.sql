-- Issue #12: user-triggered "Normalize with LLM" merchant/category pass.
--
-- One row per transaction the LLM corrected. This table is deliberately the
-- *only* new state the feature introduces, and it carries three jobs at once:
--
--   1. Undo log. Every `prev_*` column is the value that was there before the
--      pass overwrote it, so a single correction or a whole run can be put
--      back exactly. This is what makes "apply directly" safe.
--   2. Audit trail. `llm_confidence` records how sure the model was, shown
--      next to the transaction so a low-confidence rewrite is visible rather
--      than silent.
--   3. Provenance for the learned rule. `learned_rule_id` ties the correction
--      to the `pattern_rules` row synthesized from it, so reverting the
--      correction can also retire the rule it taught.
--
-- Note there is no separate "run checkpoint" table. Resume is derived, not
-- stored: the work queue is recomputed from merchant confidence each time the
-- pass starts, and an already-corrected transaction now resolves to a
-- user-sourced merchant, scores above the threshold, and drops out of the
-- queue on its own. A run interrupted by an app close simply picks up where
-- it left off next time it is started.
CREATE TABLE merchant_llm_corrections (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    transaction_id TEXT NOT NULL REFERENCES transactions(id),
    observation_id TEXT,

    prev_merchant_entity_id TEXT,
    prev_merchant_display_name TEXT,
    prev_merchant_normalized_name TEXT,
    prev_category_id TEXT,

    new_merchant_entity_id TEXT,
    new_merchant_display_name TEXT,
    new_merchant_normalized_name TEXT,
    new_category_id TEXT,

    llm_confidence REAL NOT NULL DEFAULT 0,
    learned_rule_id TEXT,
    status TEXT NOT NULL DEFAULT 'applied' CHECK(status IN ('applied', 'reverted')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Run summaries and the undo-a-whole-run path both filter on this pair.
CREATE INDEX idx_merchant_llm_corrections_run ON merchant_llm_corrections(run_id, status);
-- "What did the LLM do to this transaction?", asked per row by the UI.
CREATE INDEX idx_merchant_llm_corrections_txn ON merchant_llm_corrections(transaction_id);
