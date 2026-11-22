-- Review H8: a Layer 6 job that keeps failing was replayed at every launch
-- forever. Attempts are now counted; after the cap the job is marked dead
-- (kept for diagnosis, never replayed). Its unassigned transaction stays in
-- the review queue for the user to resolve by hand.
ALTER TABLE layer6_pending_jobs ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE layer6_pending_jobs ADD COLUMN dead_at TEXT;
