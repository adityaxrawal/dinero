-- Review P5: the local session was checked only by licence commands and Gmail
-- disconnect, never by anything that wrote ledger data or ingested email, and
-- was recreated at every launch. Incident response now pauses ingestion
-- instead, so the session and its table are gone.
DROP TABLE IF EXISTS sessions;
