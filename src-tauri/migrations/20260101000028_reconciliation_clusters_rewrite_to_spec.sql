-- TASK-DB-012: user-approved rewrite to match Document 18 §4.6/§4.6a
-- exactly. The previously-built schema was a coherent but structurally
-- different design: `reconciliation_clusters.observation_id` held the
-- single triggering observation directly on the cluster row, and
-- `reconciliation_cluster_members` only tracked competing *candidates*
-- (via `transaction_id`) with no `member_role` discriminator and no
-- `observation_id` column at all. Document 18's design instead puts every
-- participant -- the incoming observation *and* every candidate -- as a
-- role-tagged row in `reconciliation_cluster_members`.
--
-- Also reconciles the three-way disagreement on `cluster_status`/`status`
-- values: Document 18 §4.6 (authoritative) says open/resolved/deferred/
-- rejected; the previous schema had a 7-value enum
-- (unreconciled/partially_reconciled/fully_reconciled/over_reconciled/
-- disputed/ambiguous_pending/resolved); Document 30's own task text says
-- a 2-value ambiguous_pending/resolved. Using Document 18's 4 values.
CREATE TABLE reconciliation_clusters_new (
    id TEXT PRIMARY KEY,
    cluster_status TEXT NOT NULL DEFAULT 'open'
        CHECK(cluster_status IN ('open', 'resolved', 'deferred', 'rejected')),
    reason TEXT,
    resolution_notes TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    resolved_at DATETIME
);
INSERT INTO reconciliation_clusters_new (id, cluster_status, resolution_notes, created_at, resolved_at)
SELECT id,
       CASE status
           WHEN 'ambiguous_pending' THEN 'open'
           WHEN 'resolved' THEN 'resolved'
           ELSE 'open'
       END,
       resolution_note,
       created_at,
       resolved_at
FROM reconciliation_clusters;
DROP TABLE reconciliation_cluster_members;
DROP TABLE reconciliation_clusters;
ALTER TABLE reconciliation_clusters_new RENAME TO reconciliation_clusters;

CREATE TABLE reconciliation_cluster_members (
    id TEXT PRIMARY KEY,
    cluster_id TEXT NOT NULL REFERENCES reconciliation_clusters(id) ON DELETE CASCADE,
    observation_id TEXT REFERENCES transaction_observations(id),
    canonical_transaction_id TEXT REFERENCES transactions(id),
    member_role TEXT NOT NULL
        CHECK(member_role IN ('incoming', 'candidate_a', 'candidate_b', 'candidate_other')),
    added_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_cluster_members_cluster ON reconciliation_cluster_members(cluster_id);
