-- Review P4: whether onboarding finished lived only in the WebView's
-- localStorage, so it survived "Start fresh" and recovery reset (an emptied
-- app skipped onboarding) and could be set although the backend save failed.
-- It now lives with the profile and is set by the same write that stores the
-- onboarding preferences.
ALTER TABLE local_profile ADD COLUMN onboarded_at TEXT;

-- Existing installs: both onboarding exits (connect Gmail, skip Gmail) record
-- one of these consent events just before saving preferences.
UPDATE local_profile SET onboarded_at = datetime('now')
WHERE EXISTS (
    SELECT 1 FROM consent_events
    WHERE event_type IN ('gmail_oauth_consent', 'onboarding_disclosure')
);
