use async_trait::async_trait;
use fingest_kernel::{CategoryRef, DateRange, EventEnvelope, PortError};

use crate::{
    budget::{Budget, BudgetWithSpent},
    saving::Saving,
};

/// Builds the outbox rows for a new budget once the adapter knows its id. The core decides
/// what is emitted; the adapter only writes it in the insert's transaction.
pub type EventsForId<'a> = &'a (dyn Fn(i32) -> Result<Vec<EventEnvelope>, PortError> + Sync);

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

    /// Inserts the budget and, in the same transaction, the outbox rows `events` builds
    /// from the new id.
    async fn insert(
        &self,
        login: &str,
        budget: &Budget,
        events: EventsForId<'_>,
    ) -> Result<i32, PortError>;

    /// Writes `events` only if a row was updated, atomically with the update.
    async fn update(&self, budget: &Budget, events: &[EventEnvelope]) -> Result<u64, PortError>;

    /// Writes `events` only if a row was deleted, atomically with the delete.
    async fn delete(&self, budget_id: i32, events: &[EventEnvelope]) -> Result<u64, PortError>;
}

/// Savings persistence. No HTTP surface — see [`crate::saving::Saving`].
#[async_trait]
pub trait SavingRepository: Send + Sync {
    async fn list_for_owner(&self, login: &str) -> Result<Vec<Saving>, PortError>;

    async fn insert(&self, login: &str, saving: &Saving) -> Result<i32, PortError>;

    async fn delete(&self, saving_id: i32) -> Result<u64, PortError>;
}
