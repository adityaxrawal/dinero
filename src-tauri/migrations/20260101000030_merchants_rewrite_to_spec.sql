-- TASK-DB-013: the existing `merchants`/`merchant_aliases` tables (migration
-- 003) predate Document 18 and deviate from Document 18 SS4.10/SS4.11:
-- merchants had category_id/logo_url/website (none specified by Document 18,
-- which explicitly notes even the similar `approval_status` field was
-- deliberately removed as dead code) but was missing normalized_name/source;
-- merchant_aliases used a single `alias` column and `merchant_id` FK instead
-- of the raw/normalized split and `merchant_entity_id` naming Document 18
-- uses consistently everywhere else merchants is referenced (transactions.
-- merchant_entity_id, recurring_payments.merchant_entity_id).
--
-- Checked every real usage first: MerchantsRow/MerchantAliasesRow and the
-- merchants::* functions are only referenced from db/merchants.rs itself and
-- db/tests.rs -- no frontend or cross-module consumer -- so this is a clean
-- rewrite with no downstream breakage.
--
-- No production databases exist yet (pre-launch), and migration 003's seed
-- rows are the only rows this table has ever held -- so this drops and
-- recreates outright rather than carrying old rows forward, then reseeds
-- explicitly below. (Carrying rows forward via `id` while also reseeding the
-- same `id`s would collide on both the primary key and the new
-- normalized_name UNIQUE constraint.)
DROP TABLE merchant_aliases;
DROP TABLE merchants;
CREATE TABLE merchants (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL UNIQUE,
    source TEXT NOT NULL DEFAULT 'system' CHECK(source IN ('user', 'system')),
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE merchant_aliases (
    id TEXT PRIMARY KEY,
    merchant_entity_id TEXT NOT NULL REFERENCES merchants(id),
    alias_raw TEXT NOT NULL,
    alias_normalized TEXT NOT NULL UNIQUE,
    country_code TEXT,
    issuer_name TEXT,
    confidence REAL NOT NULL DEFAULT 1.0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Seed: standard Indian merchant aliases (Document 30 TASK-DB-013).
INSERT INTO merchants (id, name, normalized_name, source, is_deleted, created_at, updated_at) VALUES
('merch_amazon', 'Amazon', 'AMAZON', 'system', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_swiggy', 'Swiggy', 'SWIGGY', 'system', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_uber', 'Uber', 'UBER', 'system', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_starbucks', 'Starbucks', 'STARBUCKS', 'system', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_netflix', 'Netflix', 'NETFLIX', 'system', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);

INSERT INTO merchant_aliases (id, merchant_entity_id, alias_raw, alias_normalized, confidence, created_at) VALUES
('alias_amz1', 'merch_amazon', 'AMZN', 'AMZN', 1.0, CURRENT_TIMESTAMP),
('alias_amz2', 'merch_amazon', 'Amazon Pay India', 'AMAZON PAY INDIA', 1.0, CURRENT_TIMESTAMP),
('alias_amz3', 'merch_amazon', 'Amazon.in', 'AMAZON.IN', 1.0, CURRENT_TIMESTAMP),
('alias_swg1', 'merch_swiggy', 'SWIGGY*BANGALORE', 'SWIGGY BANGALORE', 1.0, CURRENT_TIMESTAMP),
('alias_swg2', 'merch_swiggy', 'Swiggy Ltd', 'SWIGGY LTD', 1.0, CURRENT_TIMESTAMP),
('alias_swg3', 'merch_swiggy', 'Swiggy Bundl Tech', 'SWIGGY BUNDL TECH', 1.0, CURRENT_TIMESTAMP),
('alias_ubr1', 'merch_uber', 'Uber India Systems', 'UBER INDIA SYSTEMS', 1.0, CURRENT_TIMESTAMP),
('alias_ubr2', 'merch_uber', 'Uber Rides', 'UBER RIDES', 1.0, CURRENT_TIMESTAMP),
('alias_sbx1', 'merch_starbucks', 'Starbucks Coffee', 'STARBUCKS COFFEE', 1.0, CURRENT_TIMESTAMP),
('alias_nflx', 'merch_netflix', 'Netflix Entertainment', 'NETFLIX ENTERTAINMENT', 1.0, CURRENT_TIMESTAMP);
