CREATE TABLE IF NOT EXISTS merchants (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    category_id TEXT REFERENCES categories(id),
    logo_url TEXT,
    website TEXT,
    is_deleted BOOLEAN DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS merchant_aliases (
    id TEXT PRIMARY KEY,
    merchant_id TEXT REFERENCES merchants(id),
    alias TEXT NOT NULL UNIQUE,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Merchants Seed
INSERT INTO merchants (id, name, is_deleted, created_at, updated_at) VALUES
('merch_amazon', 'Amazon', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_swiggy', 'Swiggy', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_uber', 'Uber', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_starbucks', 'Starbucks', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP),
('merch_netflix', 'Netflix', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);

-- Merchant Aliases
INSERT INTO merchant_aliases (id, merchant_id, alias, created_at) VALUES
('alias_amz1', 'merch_amazon', 'AMZN', CURRENT_TIMESTAMP),
('alias_amz2', 'merch_amazon', 'AMAZON PAY INDIA', CURRENT_TIMESTAMP),
('alias_amz3', 'merch_amazon', 'AMAZON.IN', CURRENT_TIMESTAMP),
('alias_swg1', 'merch_swiggy', 'SWIGGY BUNDL TECH', CURRENT_TIMESTAMP),
('alias_swg2', 'merch_swiggy', 'SWIGGY', CURRENT_TIMESTAMP),
('alias_ubr1', 'merch_uber', 'UBER INDIA SYSTEMS', CURRENT_TIMESTAMP),
('alias_ubr2', 'merch_uber', 'UBER RIDES', CURRENT_TIMESTAMP),
('alias_sbx1', 'merch_starbucks', 'STARBUCKS COFFEE', CURRENT_TIMESTAMP),
('alias_nflx', 'merch_netflix', 'NETFLIX ENTERTAINMENT', CURRENT_TIMESTAMP);
