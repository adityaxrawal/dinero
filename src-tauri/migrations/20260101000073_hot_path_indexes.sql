-- Audit C-2: hot lookups that were full table scans.
-- Observations by their canonical transaction (reconciliation same-source
-- check, retention, scan-cancel rollback, transaction detail).
CREATE INDEX IF NOT EXISTS idx_obs_canonical
    ON transaction_observations(canonical_transaction_id);
-- Dashboard and report date-range filters. Date only: leading with the
-- low-cardinality is_deleted made the planner prefer this over the merchant
-- and instrument indexes for queries those serve far better.
CREATE INDEX IF NOT EXISTS idx_transactions_event_time
    ON transactions(best_event_time);
-- Review-queue, cluster and Layer-6 lookups by observation.
CREATE INDEX IF NOT EXISTS idx_unassigned_observation
    ON unassigned_transactions(observation_id);
CREATE INDEX IF NOT EXISTS idx_cluster_members_observation
    ON reconciliation_cluster_members(observation_id);
CREATE INDEX IF NOT EXISTS idx_layer6_jobs_observation
    ON layer6_pending_jobs(observation_id);
