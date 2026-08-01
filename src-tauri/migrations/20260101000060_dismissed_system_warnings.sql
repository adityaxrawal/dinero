-- audit_07 #10: `system_warning` events were fire-and-forget. Dismissing one
-- cleared it from an in-memory registry only, so a *structural* condition
-- (a machine that is simply always below the RAM threshold) re-prompted on
-- every single launch, forever, with no way for the user to say "I know".
--
-- `message_hash` rather than a bare type key: a dismissal must not silence a
-- warning whose content has materially changed. "Low RAM: 9 GB free" and
-- "Low RAM: 1 GB free" share a `warning_type` but are different statements,
-- and the second one needs to be seen even if the first was waved away.
--
-- Critical warnings are never recorded here — see `emit_system_warning`. They
-- block functionality, so silencing one would hide a lockout rather than
-- reduce noise.
CREATE TABLE IF NOT EXISTS dismissed_system_warnings (
    warning_type TEXT PRIMARY KEY,
    message_hash TEXT NOT NULL,
    dismissed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
