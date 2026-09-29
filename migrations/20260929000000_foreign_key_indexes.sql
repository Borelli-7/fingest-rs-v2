-- Postgres indexes the referenced side of a foreign key, never the referencing side. Only
-- primary keys were indexed, so every per-wallet read, per-owner list and cascading delete
-- scanned the whole table across all accounts. Each index below covers one foreign key (or
-- owner filter) as its leading columns.
--
-- On a large existing database, create these by hand with CREATE INDEX CONCURRENTLY and the
-- same names before deploying; IF NOT EXISTS then makes this migration a no-op.

-- list_expenses / highest_expense filter by wallet and date; wallet delete cascades here.
CREATE INDEX IF NOT EXISTS idx_expense_wallet_date ON expense (wallet_id, date);

-- Category delete (FK check) and rename (ON UPDATE CASCADE).
CREATE INDEX IF NOT EXISTS idx_expense_category ON expense (category_name, category_profit);

-- The primary key leads with account_login, so lookups and cascades by wallet_id missed it.
CREATE INDEX IF NOT EXISTS idx_account_wallet_wallet ON account_wallet (wallet_id);

CREATE INDEX IF NOT EXISTS idx_budget_account ON budget (account_login);

CREATE INDEX IF NOT EXISTS idx_budget_category ON budget (category_name, category_profit);

CREATE INDEX IF NOT EXISTS idx_saving_account ON saving (account_login);
