-- Audit F5-4 / Phase 2 (Wave 4): a persistent record of what background work
-- did -- scan runs and statement imports -- so the outcome survives a restart.
-- The notification centre kept this in memory only. Pruned to the newest 200.
CREATE TABLE activity_log (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK(kind IN ('scan', 'statement')),
    outcome TEXT NOT NULL,
    summary TEXT NOT NULL,
    details_json TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_activity_log_created ON activity_log(created_at);
