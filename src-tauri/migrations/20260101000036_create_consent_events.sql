-- TASK-AUTH-003: Document 18 §4.21a specifies a dedicated `consent_events`
-- table, separate from `audit_log`, backing the DPDP-relevant Consent
-- History viewer (Document 06 §7). This was flagged but deliberately left
-- unresolved during TASK-DB-009 ("that's TASK-AUTH-003's scope") -- the
-- existing code instead recorded consent as audit_log rows with
-- resource_type = 'consent'. Resolving it now: Document 19 §5.6 confirms
-- the same design (`auth_get_consent_history` reads this table).
CREATE TABLE consent_events (
    id TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    disclosure_text TEXT NOT NULL,
    consented_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    withdrawn_at DATETIME
);
