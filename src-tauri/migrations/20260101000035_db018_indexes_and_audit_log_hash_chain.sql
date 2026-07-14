-- TASK-DB-018: recurring_payments/sessions/audit_log/license_state all
-- already matched Document 18 §4.14/§4.20/§4.21/§4.22 field-for-field
-- (recurring_payments has one extra, tested, working `updated_at` trigger
-- column beyond Document 18's field list -- left as-is since
-- test_recurring_payments_crud genuinely exercises it, unlike the untested
-- cosmetic columns removed in TASK-DB-013/014). Missing pieces:
CREATE INDEX idx_recurring_payments_merchant_instrument ON recurring_payments(merchant_entity_id, instrument_id);
CREATE INDEX idx_sessions_revoked_at ON sessions(revoked_at);
CREATE INDEX idx_sessions_device_fingerprint ON sessions(device_fingerprint);
CREATE INDEX idx_audit_log_created_at ON audit_log(created_at DESC);
CREATE INDEX idx_audit_log_resource ON audit_log(resource_type, resource_id);

-- §4.21's tamper-evidence hash chain (Aditya's decision resolving
-- Documentation Audit findings D-02/E-04): the immutability trigger only
-- guards against mutation through the IPC/rusqlite path -- it does nothing
-- against direct SQLite file access. prev_hash/row_hash let a verification
-- pass detect any row edited or deleted out-of-band.
ALTER TABLE audit_log ADD COLUMN prev_hash TEXT NOT NULL DEFAULT '0000000000000000000000000000000000000000000000000000000000000000';
ALTER TABLE audit_log ADD COLUMN row_hash TEXT NOT NULL DEFAULT '';

-- license_state.billing_interval_cached (added in migration 017) was never
-- given the CHECK(monthly/annual) Document 18's DDL specifies for it.
CREATE TABLE license_state_new (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    license_jwt TEXT NOT NULL,
    subscription_status_cached TEXT NOT NULL
      CHECK(subscription_status_cached IN (
        'anonymous','trialing','active','past_due','grace','locked','canceled','expired'
      )),
    plan_id_cached TEXT,
    billing_interval_cached TEXT CHECK(billing_interval_cached IN ('monthly', 'annual')),
    current_period_end_cached DATETIME,
    jwt_expires_at DATETIME NOT NULL,
    last_server_validated_at DATETIME,
    last_known_valid_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    device_fingerprint TEXT,
    source TEXT NOT NULL DEFAULT 'server_fresh'
      CHECK(source IN ('server_fresh','cached')),
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO license_state_new SELECT
    id, license_jwt, subscription_status_cached, plan_id_cached, billing_interval_cached,
    current_period_end_cached, jwt_expires_at, last_server_validated_at, last_known_valid_time,
    device_fingerprint, source, updated_at
FROM license_state;
DROP TABLE license_state;
ALTER TABLE license_state_new RENAME TO license_state;
CREATE TRIGGER IF NOT EXISTS updated_at_trigger_license_state
AFTER UPDATE ON license_state FOR EACH ROW
BEGIN UPDATE license_state SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;
