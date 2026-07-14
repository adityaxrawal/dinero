CREATE TRIGGER IF NOT EXISTS updated_at_trigger_local_profile
AFTER UPDATE ON local_profile FOR EACH ROW
BEGIN UPDATE local_profile SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_connected_accounts
AFTER UPDATE ON connected_accounts FOR EACH ROW
BEGIN UPDATE connected_accounts SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_instruments
AFTER UPDATE ON instruments FOR EACH ROW
BEGIN UPDATE instruments SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_transactions
AFTER UPDATE ON transactions FOR EACH ROW
BEGIN UPDATE transactions SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_statements
AFTER UPDATE ON statements FOR EACH ROW
BEGIN UPDATE statements SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_transaction_observations
AFTER UPDATE ON transaction_observations FOR EACH ROW
BEGIN UPDATE transaction_observations SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_reconciliation_clusters
AFTER UPDATE ON reconciliation_clusters FOR EACH ROW
BEGIN UPDATE reconciliation_clusters SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_merchants
AFTER UPDATE ON merchants FOR EACH ROW
BEGIN UPDATE merchants SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;

CREATE TRIGGER IF NOT EXISTS updated_at_trigger_categories
AFTER UPDATE ON categories FOR EACH ROW
BEGIN UPDATE categories SET updated_at = CURRENT_TIMESTAMP WHERE id = old.id; END;
