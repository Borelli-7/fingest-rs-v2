use async_trait::async_trait;
use fingest_kernel::{DateRange, PortError};
use fingest_wallets_core::{Expense, Wallet, WalletReader};
use sqlx::PgPool;

use crate::mapping::{expense, to_port_error, wallet};

pub struct PgWalletReader {
    pool: PgPool,
}

impl PgWalletReader {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WalletReader for PgWalletReader {
    async fn owner_exists(&self, login: &str) -> Result<bool, PortError> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM account WHERE login = $1) AS "exists!""#,
            login
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn list_for_owner(&self, login: &str) -> Result<Vec<Wallet>, PortError> {
        let rows = sqlx::query!(
            r#"SELECT w.id, w.name, w.amount_amount, w.amount_currency
               FROM wallet w
               JOIN account_wallet aw ON aw.wallet_id = w.id
               WHERE aw.account_login = $1
               ORDER BY w.id"#,
            login
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_port_error)?;

        rows.into_iter()
            .map(|row| wallet(row.id, row.name, row.amount_amount, row.amount_currency))
            .collect()
    }

    async fn find_owned(&self, login: &str, wallet_id: i32) -> Result<Option<Wallet>, PortError> {
        let row = sqlx::query!(
            r#"SELECT w.id, w.name, w.amount_amount, w.amount_currency
               FROM wallet w
               JOIN account_wallet aw ON aw.wallet_id = w.id
               WHERE aw.account_login = $1 AND w.id = $2"#,
            login,
            wallet_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_port_error)?;

        row.map(|row| wallet(row.id, row.name, row.amount_amount, row.amount_currency))
            .transpose()
    }

    async fn wallet_exists(&self, wallet_id: i32) -> Result<bool, PortError> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM wallet WHERE id = $1) AS "exists!""#,
            wallet_id
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn list_expenses(
        &self,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Vec<Expense>, PortError> {
        let rows = sqlx::query!(
            r#"SELECT id, amount_amount, amount_currency, date, description,
                      category_name, category_profit
               FROM expense
               WHERE wallet_id = $1 AND date BETWEEN $2 AND $3
               ORDER BY date, id"#,
            wallet_id,
            range.start,
            range.end
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_port_error)?;

        rows.into_iter()
            .map(|row| {
                expense(
                    row.id,
                    row.amount_amount,
                    row.amount_currency,
                    row.date,
                    row.description,
                    row.category_name,
                    row.category_profit,
                )
            })
            .collect()
    }

    async fn highest_expense(
        &self,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Option<Expense>, PortError> {
        let row = sqlx::query!(
            r#"SELECT id, amount_amount, amount_currency, date, description,
                      category_name, category_profit
               FROM expense
               WHERE wallet_id = $1
                 AND date BETWEEN $2 AND $3
                 AND category_profit = false
               ORDER BY amount_amount DESC, id
               LIMIT 1"#,
            wallet_id,
            range.start,
            range.end
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_port_error)?;

        row.map(|row| {
            expense(
                row.id,
                row.amount_amount,
                row.amount_currency,
                row.date,
                row.description,
                row.category_name,
                row.category_profit,
            )
        })
        .transpose()
    }

    async fn find_expense(
        &self,
        wallet_id: i32,
        expense_id: i32,
    ) -> Result<Option<Expense>, PortError> {
        let row = sqlx::query!(
            r#"SELECT id, amount_amount, amount_currency, date, description,
                      category_name, category_profit
               FROM expense
               WHERE wallet_id = $1 AND id = $2"#,
            wallet_id,
            expense_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_port_error)?;

        row.map(|row| {
            expense(
                row.id,
                row.amount_amount,
                row.amount_currency,
                row.date,
                row.description,
                row.category_name,
                row.category_profit,
            )
        })
        .transpose()
    }
}
