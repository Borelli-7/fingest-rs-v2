-- Sample data for existing tables

-- Insert some default categories
INSERT INTO category (name, profit) VALUES
('Food', false),
('Transport', false),
('Entertainment', false),
('Education', false),
('Health', false),
('Housing', false),
('Clothing', false),
('Other', false),
('Salary', true),
('Gift', true),
('Investment', true),
('Other', true)
ON CONFLICT DO NOTHING;

-- Insert default admin account
--
-- Passwords are bcrypt hashes (cost 12) of development-only credentials:
--   admin / password123   (admin)
--   user1 / user123
--   user2 / user456
--
-- v1 seeded these columns in plaintext while login called bcrypt::verify, so no
-- sample account could ever authenticate.
INSERT INTO account (login, first_name, last_name, password, admin) VALUES
('admin', 'Admin', 'User', '$2b$12$6bElbYoERmMOp1tTas9qQeFdDeWEEa53me2grUXNXbpKVrgx2HWsa', true),
('user1', 'John', 'Doe', '$2b$12$/iKqLMLGOp71oZTtoSw1LOgizw/HkCUULMrzfi.yF5SLpATXiVugK', false),
('user2', 'Jane', 'Smith', '$2b$12$s4FWvZCdZGWUPjKK97yvS.yVv2eLfncP8inckkjCJJOX.x6Uuw5AO', false)
ON CONFLICT DO NOTHING;

-- Insert default wallets
INSERT INTO wallet (name, amount_amount, amount_currency) VALUES
('Main Wallet', 1000.00, 'USD'),
('Savings', 5000.00, 'USD'),
('Travel Fund', 2500.00, 'EUR'),
('Emergency Fund', 3000.00, 'USD')
ON CONFLICT DO NOTHING;

-- Connect accounts with wallets (Note: We need to use the ID returned from the wallet insert)
INSERT INTO account_wallet (account_login, wallet_id)
SELECT 'admin', id FROM wallet WHERE name = 'Main Wallet'
ON CONFLICT DO NOTHING;

INSERT INTO account_wallet (account_login, wallet_id)
SELECT 'admin', id FROM wallet WHERE name = 'Savings'
ON CONFLICT DO NOTHING;

INSERT INTO account_wallet (account_login, wallet_id)
SELECT 'user1', id FROM wallet WHERE name = 'Travel Fund'
ON CONFLICT DO NOTHING;

INSERT INTO account_wallet (account_login, wallet_id)
SELECT 'user2', id FROM wallet WHERE name = 'Emergency Fund'
ON CONFLICT DO NOTHING;

-- Insert some sample expenses
INSERT INTO expense (wallet_id, amount_amount, amount_currency, date, description, category_name, category_profit) VALUES
((SELECT id FROM wallet WHERE name = 'Main Wallet'), 50.00, 'USD', CURRENT_DATE - INTERVAL '5 days', 'Grocery shopping', 'Food', false),
((SELECT id FROM wallet WHERE name = 'Main Wallet'), 25.00, 'USD', CURRENT_DATE - INTERVAL '3 days', 'Bus ticket', 'Transport', false),
((SELECT id FROM wallet WHERE name = 'Travel Fund'), 150.00, 'EUR', CURRENT_DATE - INTERVAL '7 days', 'Hotel reservation', 'Housing', false),
((SELECT id FROM wallet WHERE name = 'Main Wallet'), 1200.00, 'USD', CURRENT_DATE - INTERVAL '1 day', 'Monthly salary', 'Salary', true)
ON CONFLICT DO NOTHING;

-- Insert sample budgets
INSERT INTO budget (account_login, category_name, category_profit, total_amount, total_currency, start_date, end_date) VALUES
('admin', 'Food', false, 300.00, 'USD', CURRENT_DATE - INTERVAL '15 days', CURRENT_DATE + INTERVAL '15 days'),
('admin', 'Transport', false, 100.00, 'USD', CURRENT_DATE - INTERVAL '15 days', CURRENT_DATE + INTERVAL '15 days'),
('user1', 'Entertainment', false, 200.00, 'EUR', CURRENT_DATE, CURRENT_DATE + INTERVAL '30 days')
ON CONFLICT DO NOTHING;

-- Insert sample savings goals
INSERT INTO saving (account_login, name, goal_amount, goal_currency, current_amount, current_currency, start_date, end_date) VALUES
('admin', 'New Car', 20000.00, 'USD', 5000.00, 'USD', CURRENT_DATE - INTERVAL '60 days', CURRENT_DATE + INTERVAL '305 days'),
('admin', 'Vacation', 3000.00, 'USD', 1500.00, 'USD', CURRENT_DATE - INTERVAL '30 days', CURRENT_DATE + INTERVAL '60 days'),
('user1', 'New Laptop', 1500.00, 'EUR', 500.00, 'EUR', CURRENT_DATE - INTERVAL '15 days', CURRENT_DATE + INTERVAL '75 days')
ON CONFLICT DO NOTHING;
