use fingest_kernel::{DomainError, PortError};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WalletsError {
    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Internal(String),

    #[error(transparent)]
    Port(#[from] PortError),
}

impl From<DomainError> for WalletsError {
    fn from(err: DomainError) -> Self {
        match err {
            // A currency clash is a client mistake, not a malformed field.
            DomainError::CurrencyMismatch { .. } => Self::BadRequest(err.to_string()),
            other => Self::Validation(other.to_string()),
        }
    }
}

impl WalletsError {
    /// v1: `"User with login '{login}' not found"`
    pub fn user_not_found(login: &str) -> Self {
        Self::NotFound(format!("User with login '{login}' not found"))
    }

    /// v1: `"Wallet with id {id} not found for user {login}"`
    ///
    /// A wallet owned by someone else is reported as missing rather than forbidden, so the
    /// response does not confirm that the id exists.
    pub fn wallet_not_found_for_user(wallet_id: i32, login: &str) -> Self {
        Self::NotFound(format!(
            "Wallet with id {wallet_id} not found for user {login}"
        ))
    }

    /// v1: `"Wallet with id {id} not found"`
    pub fn wallet_not_found(wallet_id: i32) -> Self {
        Self::NotFound(format!("Wallet with id {wallet_id} not found"))
    }

    /// v1: `"The expense with id {id} does not exist"`
    pub fn expense_not_found(expense_id: i32) -> Self {
        Self::NotFound(format!("The expense with id {expense_id} does not exist"))
    }

    /// v1: `"Category {name} with profit={profit} does not exist"`, returned as 400.
    pub fn unknown_category(name: &str, profit: bool) -> Self {
        Self::BadRequest(format!(
            "Category {name} with profit={profit} does not exist"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_match_v1_wording() {
        assert_eq!(
            WalletsError::wallet_not_found_for_user(3, "bob").to_string(),
            "Wallet with id 3 not found for user bob"
        );
        assert_eq!(
            WalletsError::expense_not_found(9).to_string(),
            "The expense with id 9 does not exist"
        );
        assert_eq!(
            WalletsError::unknown_category("Food", false).to_string(),
            "Category Food with profit=false does not exist"
        );
        assert_eq!(
            WalletsError::user_not_found("bob").to_string(),
            "User with login 'bob' not found"
        );
    }

    #[test]
    fn a_currency_clash_is_a_bad_request_not_a_validation_error() {
        let err: WalletsError = DomainError::CurrencyMismatch {
            expected: "PLN".into(),
            actual: "USD".into(),
        }
        .into();

        assert!(matches!(err, WalletsError::BadRequest(_)));
    }

    #[test]
    fn other_domain_errors_are_validation_failures() {
        let err: WalletsError = DomainError::NegativeAmount.into();
        assert!(matches!(err, WalletsError::Validation(_)));
    }
}
