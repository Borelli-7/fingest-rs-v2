CREATE TABLE IF NOT EXISTS category (
    name VARCHAR(255) NOT NULL,
    profit BOOLEAN NOT NULL,
    PRIMARY KEY (name, profit)
);

CREATE TABLE IF NOT EXISTS account (
    login VARCHAR(255) PRIMARY KEY,
    first_name VARCHAR(255),
    last_name VARCHAR(255),
    password TEXT,
    admin BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS wallet (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    amount_amount NUMERIC(19,2) NOT NULL,
    amount_currency VARCHAR(3) NOT NULL
);

CREATE TABLE IF NOT EXISTS account_wallet (
    account_login VARCHAR(255) NOT NULL REFERENCES account(login) ON DELETE CASCADE,
    wallet_id INTEGER NOT NULL REFERENCES wallet(id) ON DELETE CASCADE,
    PRIMARY KEY (account_login, wallet_id)
);

CREATE TABLE IF NOT EXISTS expense (
    id SERIAL PRIMARY KEY,
    wallet_id INTEGER NOT NULL REFERENCES wallet(id) ON DELETE CASCADE,
    amount_amount NUMERIC(19,2) NOT NULL,
    amount_currency VARCHAR(3) NOT NULL,
    date DATE NOT NULL,
    description TEXT NOT NULL,
    category_name VARCHAR(255) NOT NULL,
    category_profit BOOLEAN NOT NULL,
    FOREIGN KEY (category_name, category_profit) REFERENCES category(name, profit)
);

CREATE TABLE IF NOT EXISTS budget (
    id SERIAL PRIMARY KEY,
    account_login VARCHAR(255) NOT NULL REFERENCES account(login) ON DELETE CASCADE,
    category_name VARCHAR(255) NOT NULL,
    category_profit BOOLEAN NOT NULL,
    total_amount NUMERIC(19,2) NOT NULL,
    total_currency VARCHAR(3) NOT NULL,
    start_date DATE NOT NULL,
    end_date DATE NOT NULL,
    FOREIGN KEY (category_name, category_profit) REFERENCES category(name, profit)
);

CREATE TABLE IF NOT EXISTS saving (
    id SERIAL PRIMARY KEY,
    account_login VARCHAR(255) NOT NULL REFERENCES account(login) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    goal_amount NUMERIC(19,2) NOT NULL,
    goal_currency VARCHAR(3) NOT NULL,
    current_amount NUMERIC(19,2) NOT NULL,
    current_currency VARCHAR(3) NOT NULL,
    start_date DATE NOT NULL,
    end_date DATE NOT NULL
);

