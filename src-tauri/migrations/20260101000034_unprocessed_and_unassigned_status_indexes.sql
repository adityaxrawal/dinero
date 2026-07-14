-- TASK-DB-017: processing_checkpoints/unprocessed_statements/unassigned_transactions
-- already matched Document 18 §4.15/§4.16/§4.17 exactly, including the
-- UNIQUE(job_type, job_key) constraint and the UPSERT-idempotency and
-- retry-lifecycle acceptance tests. Only the 2 status indexes Document 18
-- §6 lists were missing.
CREATE INDEX idx_unprocessed_statements_status ON unprocessed_statements(status);
CREATE INDEX idx_unassigned_transactions_status ON unassigned_transactions(status);
