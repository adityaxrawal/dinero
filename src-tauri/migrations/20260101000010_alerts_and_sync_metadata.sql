CREATE TABLE IF NOT EXISTS sync_metadata (
    bank_name TEXT PRIMARY KEY,
    last_synced_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS alerts (
    alert_id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    message TEXT NOT NULL,
    related_cluster_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_sync_metadata
AFTER UPDATE ON sync_metadata FOR EACH ROW
BEGIN UPDATE sync_metadata SET updated_at = CURRENT_TIMESTAMP WHERE bank_name = old.bank_name; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_alerts
AFTER UPDATE ON alerts FOR EACH ROW
BEGIN UPDATE alerts SET updated_at = CURRENT_TIMESTAMP WHERE alert_id = old.alert_id; END;
