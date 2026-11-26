-- Owner decision Q-8 (2026-11-26): every rejected email is listed under
-- "Skipped emails" (sender, subject, reason) for its 30-day ignore window, so a
-- wrongly rejected bank sender can be noticed and trusted.
ALTER TABLE ignored_messages ADD COLUMN sender_domain TEXT;
ALTER TABLE ignored_messages ADD COLUMN gate TEXT;
