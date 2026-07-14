-- Document 18 SS6 indexing strategy lists reconciliation_clusters:
-- (cluster_status) -- "Queue/review list" -- missed in migration 028's
-- rewrite, which only added reconciliation_cluster_members(cluster_id).
CREATE INDEX idx_reconciliation_clusters_status ON reconciliation_clusters(cluster_status);
