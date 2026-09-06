//! Wallets bounded context.
//!
//! Owns wallets and their entries. The wallet aggregate is the consistency boundary for
//! its balance: every mutation returns the delta to persist, and all writes run inside a
//! single [`port::UnitOfWork`] transaction alongside their outbox rows.

pub mod error;
pub mod expense;
pub mod port;
pub mod service;
pub mod summary;
pub mod wallet;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use error::WalletsError;
pub use expense::Expense;
pub use port::{UnitOfWork, WalletReader, WalletTx};
pub use service::{ExpensePatch, NewExpense, WalletPatch, WalletService};
pub use summary::{Summary, count_by_category};
pub use wallet::Wallet;
