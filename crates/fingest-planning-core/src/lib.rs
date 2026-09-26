//! Planning bounded context.
//!
//! Owns budgets and savings goals. Savings are modelled and persisted but not exposed over
//! HTTP, matching v1's surface.

pub mod budget;
pub mod error;
pub mod port;
pub mod saving;
pub mod service;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use budget::{Budget, BudgetWithSpent};
pub use error::PlanningError;
pub use port::{BudgetRepository, EventsForId, SavingRepository};
pub use saving::Saving;
pub use service::{BudgetPatch, BudgetService};
