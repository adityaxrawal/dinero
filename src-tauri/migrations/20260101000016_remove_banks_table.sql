-- Removes the forbidden `banks` table (Doc 15 §14.12, Doc 18 §4.2, Doc 12
-- §1.2/§10.4a) -- a bank is only the `issuer_name` partition of the unified
-- `instruments` table, never a first-class entity. `full_identifier` and
-- `billing_cycle_day` (also added in the previous migration) are unrelated
-- instrument metadata and are kept.
ALTER TABLE instruments DROP COLUMN bank_id;
DROP TRIGGER IF EXISTS updated_at_trigger_banks;
DROP TABLE IF EXISTS banks;
