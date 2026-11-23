-- Follow-up to review H4: a pending ingest job that fails the same way every
-- time was replayed at every launch forever. Failed attempts are counted;
-- after the cap the job is marked dead (kept for diagnosis, never replayed),
-- matching layer6_pending_jobs.
ALTER TABLE pending_ingest_jobs ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE pending_ingest_jobs ADD COLUMN dead_at TEXT;
