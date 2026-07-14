-- Onboarding previously only wrote the historical scan range, LLM model
-- choice, and statement preference to browser localStorage -- never to the
-- backend, so they never survived a reinstall/reset and never reached the
-- same `local_profile` row the rest of the app actually reads from.
ALTER TABLE local_profile ADD COLUMN historical_scan_months INTEGER;
ALTER TABLE local_profile ADD COLUMN llm_model TEXT;
ALTER TABLE local_profile ADD COLUMN statement_preference TEXT;
