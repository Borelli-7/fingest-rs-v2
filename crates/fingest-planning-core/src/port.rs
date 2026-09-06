use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fingest_kernel::{CategoryRef, DateRange, PortError};

use crate::{
    budget::{Budget, BudgetWithSpent},
    saving::Saving,
};

#[async_trait]
pub trait BudgetRepository: Send + Sync {
    async fn owner_exists(&self, login: &str) -> Result<bool, PortError>;

    async fn category_exists(&self, category: &CategoryRef) -> Result<bool, PortError>;

    /// v1 filters on two independent windows: one bounding `start_date`, one bounding
    /// `end_date`. Spending is summed from the owner's expenses in the same query.
    async fn list_with_spent(
        &self,
        login: &str,
        start_window: &DateRange,
        end_window: &DateRange,
    ) -> Result<Vec<BudgetWithSpent>, PortError>;

    /// Returns the budget and its owning login, so the caller can authorize.
    async fn find_with_owner(&self, budget_id: i32) -> Result<Option<(Budget, String)>, PortError>;

    /// Inserts the budget and, in the same transaction, appends its `BudgetCreated` outbox
    /// row.
    ///
    /// The adapter builds the event because the budget id only exists once the row is
    /// written, and the event must carry it. `occurred_at` is supplied by the caller so the
    /// timestamp still comes from the injected clock.
    async fn insert(
        &self,
        login: &str,
        budget: &Budget,
        occurred_at: DateTime<Utc>,
    ) -> Result<i32, PortError>;

    async fn update(&self, budget: &Budget) -> Result<u64, PortError>;

    async fn delete(&self, budget_id: i32) -> Result<u64, PortError>;
}

/// Savings persistence. No HTTP surface — see [`crate::saving::Saving`].
#[async_trait]
pub trait SavingRepository: Send + Sync {
    async fn list_for_owner(&self, login: &str) -> Result<Vec<Saving>, PortError>;

    async fn insert(&self, login: &str, saving: &Saving) -> Result<i32, PortError>;

    async fn delete(&self, saving_id: i32) -> Result<u64, PortError>;
}
