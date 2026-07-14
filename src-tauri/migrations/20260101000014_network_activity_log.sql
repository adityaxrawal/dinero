CREATE TABLE IF NOT EXISTS network_activity_log (
    id TEXT PRIMARY KEY,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    method TEXT NOT NULL,
    domain TEXT NOT NULL,
    url_redacted TEXT NOT NULL,
    bytes_sent INTEGER,
    bytes_received INTEGER,
    status_code INTEGER,
    secret_fields_masked TEXT
);
