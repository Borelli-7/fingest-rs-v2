use fingest_kernel::{DomainError, PortError};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdentityError {
    /// 401. Deliberately opaque: never distinguishes "no such user" from "wrong password".
    #[error("{0}")]
    Unauthenticated(String),

    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Internal(String),

    #[error(transparent)]
    Port(#[from] PortError),
}

impl From<DomainError> for IdentityError {
    fn from(err: DomainError) -> Self {
        Self::Validation(err.to_string())
    }
}

impl IdentityError {
    /// v1's wording, and the only response a failed login may produce.
    pub fn invalid_credentials() -> Self {
        Self::Unauthenticated("Invalid credentials".to_owned())
    }

    /// v1: `"User with login '{login}' already exists"`, returned as 400 not 409.
    pub fn already_exists(login: &str) -> Self {
        Self::BadRequest(format!("User with login '{login}' already exists"))
    }

    pub fn missing_token() -> Self {
        Self::Unauthenticated("Missing Authorization header".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_failures_are_indistinguishable() {
        assert_eq!(
            IdentityError::invalid_credentials().to_string(),
            "Invalid credentials"
        );
    }

    #[test]
    fn duplicate_registration_matches_v1_wording() {
        assert_eq!(
            IdentityError::already_exists("bob").to_string(),
            "User with login 'bob' already exists"
        );
    }

    #[test]
    fn duplicate_registration_is_a_bad_request_like_v1() {
        assert!(matches!(
            IdentityError::already_exists("bob"),
            IdentityError::BadRequest(_)
        ));
    }
}
