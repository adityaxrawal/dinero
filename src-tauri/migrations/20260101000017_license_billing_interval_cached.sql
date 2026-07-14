-- Document 19 §14.1 "Field added": monthly/annual -- cached display field
-- for license_get_status, mirroring the JWT's billing_interval claim set
-- at activation.
ALTER TABLE license_state ADD COLUMN billing_interval_cached TEXT;
