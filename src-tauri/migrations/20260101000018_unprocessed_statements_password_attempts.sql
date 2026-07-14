-- Doc 22 §15.2/Doc 24 §3.5: tracks wrong-password attempts per blocked
-- statement so a 3-attempt cap can be enforced -- previously only the
-- 2.5-minute timeout was.
ALTER TABLE unprocessed_statements ADD COLUMN password_attempts INTEGER NOT NULL DEFAULT 0;
