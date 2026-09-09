//! Shared kernel for fingest-rs-v2.
//!
//! Pure domain vocabulary shared by every bounded context: value objects, domain errors,
//! the clock port and the event port. This crate performs no I/O and must never depend on
//! `sqlx`, `actix-web`, `tokio` or any adapter crate — see `tests/dependency_rule.rs`.

pub mod category_ref;
pub mod clock;
pub mod currency;
pub mod date_range;
pub mod error;
pub mod event;
pub mod money;

pub use category_ref::CategoryRef;
pub use clock::{Clock, FixedClock, SystemClock};
pub use currency::Currency;
pub use date_range::DateRange;
pub use error::{DomainError, PortError};
pub use event::{DomainEvent, EventEnvelope, EventPublisher};
pub use money::Money;
