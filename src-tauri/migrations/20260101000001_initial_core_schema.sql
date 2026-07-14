CREATE TABLE IF NOT EXISTS local_profile (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    primary_email TEXT,
    display_name TEXT,
    timezone TEXT,
    spending_limit_monthly NUMERIC,
    limit_thresholds JSONB,
    recovery_phrase_enabled BOOLEAN DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS connected_accounts (
    id TEXT PRIMARY KEY,
    profile_id INTEGER DEFAULT 1 REFERENCES local_profile(id),
    email_address TEXT,
    account_status TEXT,
    last_history_id TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS instruments (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL CHECK(type IN ('credit_card','debit_card','bank_account','UPI','NEFT','RTGS','SWIFT','upi_vpa','wallet','POS','ATM','cheque')),
    issuer_name TEXT NOT NULL,
    masked_identifier TEXT NOT NULL,
    network TEXT,
    credit_limit INTEGER,
    current_balance INTEGER,
    statement_due_date DATE,
    minimum_due INTEGER,
    bank_ifsc TEXT,
    account_type TEXT,
    upi_vpa TEXT,
    nickname TEXT,
    rewards_summary TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','inactive','closed')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    UNIQUE(type, issuer_name, masked_identifier)
);

CREATE TABLE IF NOT EXISTS transactions (
    id TEXT PRIMARY KEY,
    unique_event_id TEXT UNIQUE,
    instrument_id TEXT REFERENCES instruments(id),
    instrument_type TEXT,
    direction TEXT,
    amount NUMERIC,
    amount_minor BIGINT,
    currency CHAR(3),
    authorization_time DATETIME,
    best_event_time DATETIME,
    event_time_confidence TEXT,
    best_posting_date DATE,
    posting_date_confidence TEXT,
    merchant_display_name TEXT,
    merchant_normalized_name TEXT,
    merchant_entity_id TEXT,
    reference_id TEXT,
    location TEXT,
    original_amount_minor BIGINT,
    original_currency CHAR(3),
    exchange_rate NUMERIC,
    balance_after_transaction NUMERIC,
    status TEXT,
    match_confidence TEXT,
    source_mix TEXT,
    alert_fired BOOLEAN,
    parent_transaction_id TEXT REFERENCES transactions(id),
    transaction_subtype TEXT,
    emi_group_id TEXT,
    category_id TEXT,
    is_deleted BOOLEAN DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS statements (
    id TEXT PRIMARY KEY,
    instrument_id TEXT REFERENCES instruments(id),
    statement_type TEXT NOT NULL,
    billing_period_start DATE NOT NULL,
    billing_period_end DATE NOT NULL,
    due_date DATE,
    statement_date DATE,
    current_balance BIGINT,
    minimum_due BIGINT,
    rewards_summary_json JSONB,
    source_message_id TEXT,
    parse_status TEXT NOT NULL,
    is_duplicate BOOLEAN NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS statement_entries (
    id TEXT PRIMARY KEY,
    statement_id TEXT REFERENCES statements(id),
    row_index INTEGER,
    transaction_date DATE,
    posting_date DATE,
    description_raw TEXT,
    merchant_raw TEXT,
    merchant_normalized TEXT,
    amount NUMERIC,
    amount_minor BIGINT,
    currency CHAR(3),
    direction TEXT,
    reference_id TEXT,
    location TEXT,
    raw_row_json JSONB,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS transaction_observations (
    id TEXT PRIMARY KEY,
    canonical_transaction_id TEXT REFERENCES transactions(id),
    source_pipeline TEXT,
    source_record_id TEXT,
    source_message_id TEXT,
    source_thread_id TEXT,
    statement_id TEXT REFERENCES statements(id),
    statement_entry_id TEXT REFERENCES statement_entries(id),
    instrument_id TEXT REFERENCES instruments(id),
    direction TEXT,
    amount NUMERIC,
    amount_minor BIGINT,
    currency CHAR(3),
    event_time DATETIME,
    event_time_confidence TEXT,
    posting_date DATE,
    merchant_raw TEXT,
    merchant_normalized TEXT,
    reference_id TEXT,
    original_amount_minor BIGINT,
    original_currency CHAR(3),
    exchange_rate NUMERIC,
    balance_after_transaction NUMERIC,
    timezone_at_ingestion TEXT,
    fingerprint TEXT,
    extraction_method TEXT,
    confidence_score NUMERIC,
    raw_payload_json JSONB,
    parser_version TEXT,
    emi_total_installments INTEGER,
    emi_installment_number INTEGER,
    emi_original_amount_minor BIGINT,
    is_deleted BOOLEAN DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(source_pipeline, source_record_id),
    UNIQUE(fingerprint)
);

CREATE TABLE IF NOT EXISTS match_decisions (
    id TEXT PRIMARY KEY,
    observation_id TEXT REFERENCES transaction_observations(id),
    matched_transaction_id TEXT REFERENCES transactions(id),
    decision TEXT,
    score NUMERIC,
    rules_triggered_json JSONB,
    review_status TEXT,
    reviewed_by TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS reconciliation_clusters (
    id TEXT PRIMARY KEY,
    observation_id TEXT REFERENCES transaction_observations(id),
    status TEXT NOT NULL DEFAULT 'unreconciled' CHECK(status IN ('unreconciled','partially_reconciled','fully_reconciled','over_reconciled','disputed','ambiguous_pending','resolved')),
    total_amount_minor BIGINT,
    currency CHAR(3),
    resolution_note TEXT,
    resolved_at DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS reconciliation_cluster_members (
    id TEXT PRIMARY KEY,
    cluster_id TEXT REFERENCES reconciliation_clusters(id) ON DELETE CASCADE,
    transaction_id TEXT REFERENCES transactions(id),
    statement_entry_id TEXT REFERENCES statement_entries(id),
    added_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
