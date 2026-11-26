-- Audit F7-5: the "run this model despite low RAM" override lived in the
-- WebView's localStorage while Rust (startup::llm_ready) decided readiness, so
-- one decision had two owners. It now lives with the other LLM preferences.
ALTER TABLE local_profile ADD COLUMN llm_ram_override INTEGER NOT NULL DEFAULT 0;
