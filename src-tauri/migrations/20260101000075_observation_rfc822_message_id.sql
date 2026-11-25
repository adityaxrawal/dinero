-- Audit N-F4 (owner decision): the same bank alert delivered to two connected
-- inboxes is one email, so it must book once. Gmail message ids differ per
-- inbox; the RFC 822 Message-ID header does not. Equal Message-IDs are the
-- same email: the second copy is a duplicate, never a second observation.
-- NULL for observations without one (statements, manual, older rows).
ALTER TABLE transaction_observations ADD COLUMN rfc822_message_id TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_obs_rfc822_message_id
    ON transaction_observations(rfc822_message_id)
    WHERE rfc822_message_id IS NOT NULL;
