-- Migration 20260101000052: Fix counterparty VPA instruments improperly saved as user instruments
-- Soft delete instruments where masked_identifier or upi_vpa was populated with a counterparty/payee VPA

UPDATE instruments
SET is_deleted = 1
WHERE is_deleted = 0
  AND (type = 'upi_vpa' OR upi_vpa IS NOT NULL)
  AND (
    masked_identifier LIKE '7674036967%'
    OR masked_identifier LIKE 'saharahospital%'
    OR LOWER(masked_identifier) IN (SELECT LOWER(merchant_display_name) FROM transactions WHERE merchant_display_name IS NOT NULL)
  );

-- Unassign transactions linked to soft-deleted instruments so they can be re-linked to clean bank instruments
UPDATE transactions
SET instrument_id = NULL
WHERE instrument_id IN (SELECT id FROM instruments WHERE is_deleted = 1 AND (type = 'upi_vpa' OR upi_vpa IS NOT NULL));
