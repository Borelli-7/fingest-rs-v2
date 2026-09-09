use thiserror::Error;

/// Violations of domain invariants. Carries no transport or persistence concerns.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    #[error("Amount cannot be negative")]
    NegativeAmount,

    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    #[error("Currency mismatch: expected {expected}, got {actual}")]
    CurrencyMismatch { expected: String, actual: String },

    #[error("Invalid currency code: {0}")]
    InvalidCurrencyCode(String),

    #[error("Invalid date range: start {start} is after end {end}")]
    InvalidDateRange { start: String, end: String },

    #[error("{field} cannot be empty")]
    EmptyField { field: &'static str },

    #[error("{field} must be at least {min} characters")]
    FieldTooShort { field: &'static str, min: usize },

    #[error("{field} exceeds maximum length of {max} characters")]
    FieldTooLong { field: &'static str, max: usize },
}

/// Failure of an outbound adapter. Cores return this opaquely; they never inspect
/// driver-specific detail.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PortError {
    #[error("Storage failure: {0}")]
    Storage(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Dependency unavailable: {0}")]
    Unavailable(String),

    #[error("Encoding failure: {0}")]
    Encoding(String),
}
