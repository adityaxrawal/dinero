-- Audit F19-3: duplicate-check decisions and local-AI results are recorded in
-- the activity log too. SQLite cannot alter a CHECK, so the table is rebuilt
-- with the wider kind list; existing entries are kept.
CREATE TABLE activity_log_new (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK(kind IN ('scan', 'statement', 'cluster', 'ai')),
    outcome TEXT NOT NULL,
    summary TEXT NOT NULL,
    details_json TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO activity_log_new (id, kind, outcome, summary, details_json, created_at)
    SELECT id, kind, outcome, summary, details_json, created_at FROM activity_log;
DROP TABLE activity_log;
ALTER TABLE activity_log_new RENAME TO activity_log;
CREATE INDEX idx_activity_log_created ON activity_log(created_at);
