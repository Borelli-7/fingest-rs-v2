use async_trait::async_trait;
use fingest_kernel::{CategoryRef, EventEnvelope, Money, PortError};
use fingest_wallets_core::{Expense, UnitOfWork, Wallet, WalletTx};
use sqlx::{PgPool, Postgres, Transaction};

use crate::mapping::{expense, to_port_error, wallet};

pub struct PgUnitOfWork {
    pool: PgPool,
}

impl PgUnitOfWork {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UnitOfWork for PgUnitOfWork {
    async fn begin(&self) -> Result<Box<dyn WalletTx>, PortError> {
        let tx = self.pool.begin().await.map_err(to_port_error)?;
        Ok(Box::new(PgWalletTx { tx }))
    }
}

/// A Postgres transaction. Dropping without [`WalletTx::commit`] rolls back, which is what
/// makes every early return in the use cases safe.
pub struct PgWalletTx {
    tx: Transaction<'static, Postgres>,
}

#[async_trait]
impl WalletTx for PgWalletTx {
    async fn category_exists(&mut self, category: &CategoryRef) -> Result<bool, PortError> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(
                   SELECT 1 FROM category WHERE name = $1 AND profit = $2
               ) AS "exists!""#,
            category.name,
            category.profit
        )
        .fetch_one(&mut *self.tx)
        .await
        .map_err(to_port_error)
    }

    async fn find_owned(
        &mut self,
        login: &str,
        wallet_id: i32,
    ) -> Result<Option<Wallet>, PortError> {
        let row = sqlx::query!(
            r#"SELECT w.id, w.name, w.amount_amount, w.amount_currency
               FROM wallet w
               JOIN account_wallet aw ON aw.wallet_id = w.id
               WHERE aw.account_login = $1 AND w.id = $2
               FOR UPDATE OF w"#,
            login,
            wallet_id
        )
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(to_port_error)?;

        row.map(|row| wallet(row.id, row.name, row.amount_amount, row.amount_currency))
            .transpose()
    }

    async fn find_expense(
        &mut self,
        wallet_id: i32,
        expense_id: i32,
    ) -> Result<Option<Expense>, PortError> {
        let row = sqlx::query!(
            r#"SELECT id, amount_amount, amount_currency, date, description,
                      category_name, category_profit
               FROM expense
               WHERE wallet_id = $1 AND id = $2
               FOR UPDATE"#,
            wallet_id,
            expense_id
        )
        .fetch_optional(&mut *self.tx)
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

    async fn insert_wallet(&mut self, login: &str, new: &Wallet) -> Result<i32, PortError> {
        let id = sqlx::query_scalar!(
            r#"INSERT INTO wallet (name, amount_amount, amount_currency)
               VALUES ($1, $2, $3) RETURNING id"#,
            new.name,
            new.amount.amount,
            new.amount.currency.as_str()
        )
        .fetch_one(&mut *self.tx)
        .await
        .map_err(to_port_error)?;

        sqlx::query!(
            r#"INSERT INTO account_wallet (account_login, wallet_id) VALUES ($1, $2)"#,
            login,
            id
        )
        .execute(&mut *self.tx)
        .await
        .map_err(to_port_error)?;

        Ok(id)
    }

    async fn update_wallet(&mut self, updated: &Wallet) -> Result<u64, PortError> {
        sqlx::query!(
            r#"UPDATE wallet
               SET name = $1, amount_amount = $2, amount_currency = $3
               WHERE id = $4"#,
            updated.name,
            updated.amount.amount,
            updated.amount.currency.as_str(),
            updated.id
        )
        .execute(&mut *self.tx)
        .await
        .map(|r| r.rows_affected())
        .map_err(to_port_error)
    }

    async fn delete_wallet(&mut self, wallet_id: i32) -> Result<u64, PortError> {
        sqlx::query!(r#"DELETE FROM wallet WHERE id = $1"#, wallet_id)
            .execute(&mut *self.tx)
            .await
            .map(|r| r.rows_affected())
            .map_err(to_port_error)
    }

    async fn insert_expense(&mut self, wallet_id: i32, new: &Expense) -> Result<i32, PortError> {
        sqlx::query_scalar!(
            r#"INSERT INTO expense
               (wallet_id, amount_amount, amount_currency, date, description,
                category_name, category_profit)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING id"#,
            wallet_id,
            new.amount.amount,
            new.amount.currency.as_str(),
            new.date,
            new.description,
            new.category.name,
            new.category.profit
        )
        .fetch_one(&mut *self.tx)
        .await
        .map_err(to_port_error)
    }

    async fn update_expense(&mut self, updated: &Expense) -> Result<u64, PortError> {
        sqlx::query!(
            r#"UPDATE expense
               SET amount_amount = $1, amount_currency = $2, date = $3, description = $4,
                   category_name = $5, category_profit = $6
               WHERE id = $7"#,
            updated.amount.amount,
            updated.amount.currency.as_str(),
            updated.date,
            updated.description,
            updated.category.name,
            updated.category.profit,
            updated.id
        )
        .execute(&mut *self.tx)
        .await
        .map(|r| r.rows_affected())
        .map_err(to_port_error)
    }

    async fn delete_expense(&mut self, wallet_id: i32, expense_id: i32) -> Result<u64, PortError> {
        sqlx::query!(
            r#"DELETE FROM expense WHERE id = $1 AND wallet_id = $2"#,
            expense_id,
            wallet_id
        )
        .execute(&mut *self.tx)
        .await
        .map(|r| r.rows_affected())
        .map_err(to_port_error)
    }

    /// No currency predicate here: the aggregate has already proved the delta matches the
    /// wallet. v1's `WHERE amount_currency = $3` is exactly what let balances drift.
    async fn adjust_balance(&mut self, wallet_id: i32, delta: &Money) -> Result<(), PortError> {
        sqlx::query!(
            r#"UPDATE wallet SET amount_amount = amount_amount + $1 WHERE id = $2"#,
            delta.amount,
            wallet_id
        )
        .execute(&mut *self.tx)
        .await
        .map_err(to_port_error)?;

        Ok(())
    }

    async fn append_events(&mut self, events: &[EventEnvelope]) -> Result<(), PortError> {
        for event in events {
            sqlx::query!(
                r#"INSERT INTO outbox (aggregate, event_type, payload, occurred_at)
                   VALUES ($1, $2, $3, $4)"#,
                event.aggregate.as_str(),
                event.event_type.as_str(),
                event.payload,
                event.occurred_at
            )
            .execute(&mut *self.tx)
            .await
            .map_err(to_port_error)?;
        }

        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<(), PortError> {
        (*self).tx.commit().await.map_err(to_port_error)
    }
}
