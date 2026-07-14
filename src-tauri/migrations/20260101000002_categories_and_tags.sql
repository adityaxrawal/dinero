CREATE TABLE IF NOT EXISTS categories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    parent_id TEXT REFERENCES categories(id),
    color TEXT,
    icon TEXT,
    is_system BOOLEAN DEFAULT 0,
    is_deleted BOOLEAN DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS transaction_tags (
    transaction_id TEXT REFERENCES transactions(id),
    tag_id TEXT REFERENCES tags(id),
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (transaction_id, tag_id)
);

-- Initial Categories Seed

-- Root Level Categories
INSERT INTO categories (id, name, parent_id, color, icon, is_system, is_deleted) VALUES
('cat_food', 'Food & Dining', NULL, '#FF6B6B', 'utensils', 1, 0),
('cat_transport', 'Transportation', NULL, '#4ECDC4', 'car', 1, 0),
('cat_shopping', 'Shopping', NULL, '#45B7D1', 'shopping-bag', 1, 0),
('cat_housing', 'Housing', NULL, '#FDCB6E', 'home', 1, 0),
('cat_utilities', 'Utilities', NULL, '#6C5CE7', 'bolt', 1, 0),
('cat_entertainment', 'Entertainment', NULL, '#FF9FF3', 'film', 1, 0),
('cat_health', 'Health & Fitness', NULL, '#1DD1A1', 'heartbeat', 1, 0),
('cat_personal', 'Personal Care', NULL, '#F368E0', 'smile', 1, 0),
('cat_education', 'Education', NULL, '#2E86DE', 'graduation-cap', 1, 0),
('cat_income', 'Income', NULL, '#10AC84', 'arrow-down', 1, 0),
('cat_transfers', 'Transfers', NULL, '#576574', 'exchange-alt', 1, 0)
ON CONFLICT(name) DO UPDATE SET
  is_system = excluded.is_system,
  parent_id = excluded.parent_id,
  color = excluded.color,
  icon = excluded.icon;

-- Sub-categories for Food & Dining
INSERT INTO categories (id, name, parent_id, color, icon, is_system, is_deleted) VALUES
('cat_food_groceries', 'Groceries', 'cat_food', '#FF6B6B', 'shopping-cart', 1, 0),
('cat_food_restaurants', 'Restaurants', 'cat_food', '#FF6B6B', 'hamburger', 1, 0),
('cat_food_coffee', 'Coffee Shops', 'cat_food', '#FF6B6B', 'coffee', 1, 0)
ON CONFLICT(name) DO UPDATE SET
  is_system = excluded.is_system,
  parent_id = excluded.parent_id,
  color = excluded.color,
  icon = excluded.icon;

-- Sub-categories for Transportation
INSERT INTO categories (id, name, parent_id, color, icon, is_system, is_deleted) VALUES
('cat_transport_fuel', 'Fuel/Gas', 'cat_transport', '#4ECDC4', 'gas-pump', 1, 0),
('cat_transport_public', 'Public Transit', 'cat_transport', '#4ECDC4', 'bus', 1, 0),
('cat_transport_ride', 'Ride Sharing', 'cat_transport', '#4ECDC4', 'taxi', 1, 0)
ON CONFLICT(name) DO UPDATE SET
  is_system = excluded.is_system,
  parent_id = excluded.parent_id,
  color = excluded.color,
  icon = excluded.icon;

-- Sub-categories for Housing
INSERT INTO categories (id, name, parent_id, color, icon, is_system, is_deleted) VALUES
('cat_housing_rent', 'Rent/Mortgage', 'cat_housing', '#FDCB6E', 'building', 1, 0),
('cat_housing_maintenance', 'Maintenance', 'cat_housing', '#FDCB6E', 'tools', 1, 0)
ON CONFLICT(name) DO UPDATE SET
  is_system = excluded.is_system,
  parent_id = excluded.parent_id,
  color = excluded.color,
  icon = excluded.icon;

-- Sub-categories for Income
INSERT INTO categories (id, name, parent_id, color, icon, is_system, is_deleted) VALUES
('cat_income_salary', 'Salary', 'cat_income', '#10AC84', 'money-bill-wave', 1, 0),
('cat_income_interest', 'Interest/Dividends', 'cat_income', '#10AC84', 'chart-line', 1, 0)
ON CONFLICT(name) DO UPDATE SET
  is_system = excluded.is_system,
  parent_id = excluded.parent_id,
  color = excluded.color,
  icon = excluded.icon;
