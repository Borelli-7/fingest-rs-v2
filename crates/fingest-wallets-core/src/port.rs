use async_trait::async_trait;
use fingest_kernel::{CategoryRef, DateRange, EventEnvelope, Money, PortError};

use crate::{expense::Expense, wallet::Wallet};

/// Read-only queries. Separated from [`UnitOfWork`] so a query cannot accidentally hold a
/// transaction open.
#[async_trait]
pub trait WalletReader: Send + Sync {
    async fn owner_exists(&self, login: &str) -> Result<bool, PortError>;

    async fn list_for_owner(&self, login: &str) -> Result<Vec<Wallet>, PortError>;

    /// `None` when the wallet does not exist *or* is not owned by `login`; the caller must
    /// not distinguish the two.
    async fn find_owned(&self, login: &str, wallet_id: i32) -> Result<Option<Wallet>, PortError>;

    /// Used only to choose between 403 and 404 when a wallet is not owned.
    async fn wallet_exists(&self, wallet_id: i32) -> Result<bool, PortError>;

    async fn list_expenses(
        &self,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Vec<Expense>, PortError>;

    /// Highest *spending* entry in range; income is excluded, as in v1.
    async fn highest_expense(
        &self,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Option<Expense>, PortError>;

    async fn find_expense(
        &self,
        wallet_id: i32,
        expense_id: i32,
    ) -> Result<Option<Expense>, PortError>;
}

/// Starts a transaction.
///
/// v1 wrote an expense and then updated the wallet balance as two independent statements,
/// so a failure between them left the balance permanently wrong. Every write path now runs
/// inside one transaction that also carries the outbox rows.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn WalletTx>, PortError>;
}

/// An open transaction. Dropping without [`WalletTx::commit`] rolls back.
#[async_trait]
pub trait WalletTx: Send {
    async fn category_exists(&mut self, category: &CategoryRef) -> Result<bool, PortError>;

    async fn find_owned(
        &mut self,
        login: &str,
        wallet_id: i32,
    ) -> Result<Option<Wallet>, PortError>;

    /// Read inside the transaction so a balance delta is never computed from a copy that
    /// a concurrent update or delete has already superseded.
    async fn find_expense(
        &mut self,
        wallet_id: i32,
        expense_id: i32,
    ) -> Result<Option<Expense>, PortError>;

    async fn insert_wallet(&mut self, login: &str, wallet: &Wallet) -> Result<i32, PortError>;

    async fn update_wallet(&mut self, wallet: &Wallet) -> Result<u64, PortError>;

    async fn delete_wallet(&mut self, wallet_id: i32) -> Result<u64, PortError>;

    async fn insert_expense(&mut self, wallet_id: i32, expense: &Expense)
    -> Result<i32, PortError>;

    async fn update_expense(&mut self, expense: &Expense) -> Result<u64, PortError>;

    async fn delete_expense(&mut self, wallet_id: i32, expense_id: i32) -> Result<u64, PortError>;

    /// Applies a signed delta. The aggregate computes it; no caller invents one.
    async fn adjust_balance(&mut self, wallet_id: i32, delta: &Money) -> Result<(), PortError>;

    async fn append_events(&mut self, events: &[EventEnvelope]) -> Result<(), PortError>;

    async fn commit(self: Box<Self>) -> Result<(), PortError>;
}
