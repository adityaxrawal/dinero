-- Display-only transaction channel/rail (UPI, IMPS, NEFT, RTGS, POS, ATM,
-- wallet, internal_transfer, ecs_nach, cheque, emi, bnpl, loan). Free text,
-- no CHECK constraint, same shape as `transaction_subtype` -- purely
-- additive metadata, never consumed by reconciliation/dedup matching.
ALTER TABLE transaction_observations ADD COLUMN channel TEXT;
ALTER TABLE transactions ADD COLUMN channel TEXT;
