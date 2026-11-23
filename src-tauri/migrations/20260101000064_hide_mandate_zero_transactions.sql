-- Review M5: every mandate (standing-instruction) event used to book a ₹0
-- transaction. Mandates are now recurring-payment markers only. Existing
-- mandate-only ₹0 transactions and their observations are soft-deleted
-- (is_deleted = 1), which is reversible and leaves foreign keys intact. A
-- transaction that any non-mandate observation also points to is left alone.
UPDATE transactions
SET is_deleted = 1
WHERE COALESCE(amount_minor, 0) = 0
  AND id IN (
      SELECT canonical_transaction_id FROM transaction_observations
      WHERE extraction_method = 'mandate_event' AND canonical_transaction_id IS NOT NULL
  )
  AND id NOT IN (
      SELECT canonical_transaction_id FROM transaction_observations
      WHERE extraction_method <> 'mandate_event' AND canonical_transaction_id IS NOT NULL
  );

UPDATE transaction_observations
SET is_deleted = 1
WHERE extraction_method = 'mandate_event';
