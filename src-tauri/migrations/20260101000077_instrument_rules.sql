-- Owner decision Q-6 (2026-11-26): when an alert names the bank and card type
-- but no card number, the instrument is never guessed. After the user picks it
-- once they may confirm "always use this card for <issuer> <type> alerts".
-- One active rule per (issuer, type); consulted only when the number is absent.
CREATE TABLE instrument_rules (
    id TEXT PRIMARY KEY,
    issuer_name TEXT NOT NULL,
    instrument_type TEXT NOT NULL,
    instrument_id TEXT NOT NULL REFERENCES instruments(id),
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'inactive')),
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(issuer_name, instrument_type)
);
