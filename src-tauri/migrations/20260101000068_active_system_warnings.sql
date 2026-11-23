-- Follow-up sweep: active system warnings lived only in memory, so a warning
-- such as a failed backup disappeared on restart while its cause remained.
-- Warnings the app re-derives itself (sync watchdog, low RAM, clock skew) are
-- not stored here.
CREATE TABLE active_system_warnings (
    warning_type TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
