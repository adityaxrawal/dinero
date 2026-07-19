-- Gate 1 sender reputation/history (verified_senders.rs hardening): tracks
-- how long a sender domain has been seen and how it resolved, so Gate 1's
-- subject-based "Unknown Bank" rescue fallback (previously usable on a
-- domain's very first-ever message) can require at least one prior sighting
-- before trusting subject-line wording alone -- a brand-new domain has no
-- history yet to weigh against a spoofed subject.
CREATE TABLE IF NOT EXISTS sender_reputation (
    domain TEXT PRIMARY KEY,
    first_seen_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    message_count INTEGER NOT NULL DEFAULT 0,
    verified_pass_count INTEGER NOT NULL DEFAULT 0,
    last_verification_result TEXT NOT NULL DEFAULT ''
);

-- Doc 30-style learning loop for Gate 1 (mirrors pattern_rules' pending ->
-- active promotion for the extraction ladder's Layer 1): a domain that
-- repeatedly fails string-based verification but that a user has manually
-- confirmed is a legitimate sender can be promoted here without a registry
-- code change + app release. SenderValidator checks this table as a
-- secondary, runtime-updatable layer alongside the compiled-in registry
-- JSON, exactly the way pattern_rules layers on top of the compiled-in
-- bank_templates for extraction.
CREATE TABLE IF NOT EXISTS pending_senders (
    id TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    bank_name TEXT NOT NULL,
    classification TEXT NOT NULL DEFAULT 'transaction_candidate',
    status TEXT NOT NULL DEFAULT 'pending',
    reject_count INTEGER NOT NULL DEFAULT 1,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_pending_senders_domain ON pending_senders(domain);
CREATE INDEX IF NOT EXISTS idx_pending_senders_status ON pending_senders(status);
