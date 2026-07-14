CREATE TABLE IF NOT EXISTS recurring_payments (
    id TEXT PRIMARY KEY,
    merchant_entity_id TEXT REFERENCES merchants(id),
    instrument_id TEXT REFERENCES instruments(id),
    amount_minor BIGINT,
    currency CHAR(3),
    cadence TEXT,
    next_billing_date DATE,
    next_predicted_date DATE,
    next_predicted_amount NUMERIC,
    confidence NUMERIC,
    status TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_recurring_payments
AFTER UPDATE ON recurring_payments FOR EACH ROW
BEGIN UPDATE recurring_payments SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    device_name TEXT,
    device_fingerprint TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    revoked_at DATETIME
);

CREATE TABLE IF NOT EXISTS audit_log (
    id TEXT PRIMARY KEY,
    actor_type TEXT,
    actor_id TEXT,
    action TEXT,
    resource_type TEXT,
    resource_id TEXT,
    before_json JSONB,
    after_json JSONB,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER IF NOT EXISTS immutable_audit_log
BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'audit_log is immutable');
END;

CREATE TABLE IF NOT EXISTS license_state (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    license_jwt TEXT NOT NULL,
    subscription_status_cached TEXT NOT NULL
      CHECK(subscription_status_cached IN (
        'anonymous','trialing','active','past_due','grace','locked','canceled','expired'
      )),
    plan_id_cached TEXT,
    current_period_end_cached DATETIME,
    jwt_expires_at DATETIME NOT NULL,
    last_server_validated_at DATETIME,
    last_known_valid_time DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    device_fingerprint TEXT,
    source TEXT NOT NULL DEFAULT 'server_fresh'
      CHECK(source IN ('server_fresh','cached')),
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_license_state
AFTER UPDATE ON license_state FOR EACH ROW
BEGIN UPDATE license_state SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;
