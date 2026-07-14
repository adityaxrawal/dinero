-- TASK-DB-016: pdf_passwords/pattern_rules/feedback_log tables, CRUD, CHECK
-- constraints, and the Layer 1 pattern-rule state machine (pending->active
-- at 3 successes, active->trusted at 10, ->inactive at 3 failures or <70%
-- confidence) already matched Document 18 §4.12/§4.13/§4.19 and passed
-- their acceptance tests. Only the 3 indexes Document 18 §6 lists were
-- missing.
CREATE INDEX idx_pdf_passwords_instrument_success ON pdf_passwords(instrument_id, success_count DESC);
CREATE INDEX idx_pattern_rules_lookup ON pattern_rules(bank_name, template_hash, field_name);
CREATE INDEX idx_feedback_log_transaction ON feedback_log(transaction_id);
