-- Doc 30 TASK-OPS-009: local history for the release-readiness debug view's
-- trend chart -- each row is a point-in-time snapshot of locally-verifiable
-- aggregate metrics only (counts/rates/booleans), never per-user financial
-- data (no merchant names, amounts, or transaction content).
CREATE TABLE IF NOT EXISTS release_readiness_snapshots (
    id TEXT PRIMARY KEY,
    captured_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    metrics_json TEXT NOT NULL,
    go_no_go INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_release_readiness_snapshots_captured_at
    ON release_readiness_snapshots (captured_at);
