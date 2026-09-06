use fingest_kernel::DomainError;
use serde::{Deserialize, Serialize};

/// A user account. Deliberately carries no password: credentials travel separately so a
/// hash can never be serialised into a response by accident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub login: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub admin: bool,
}

impl Account {
    pub const MAX_LOGIN_LEN: usize = 255;

    pub fn new(
        login: impl Into<String>,
        first_name: Option<String>,
        last_name: Option<String>,
        admin: bool,
    ) -> Result<Self, DomainError> {
        let login = login.into();
        let trimmed = login.trim();

        if trimmed.is_empty() {
            return Err(DomainError::EmptyField { field: "Login" });
        }
        if trimmed.chars().count() > Self::MAX_LOGIN_LEN {
            return Err(DomainError::FieldTooLong {
                field: "Login",
                max: Self::MAX_LOGIN_LEN,
            });
        }

        Ok(Self {
            login: trimmed.to_owned(),
            first_name,
            last_name,
            admin,
        })
    }
}

/// An account plus its stored password hash. Only repositories and the login use case
/// ever see this.
#[derive(Debug, Clone)]
pub struct StoredAccount {
    pub account: Account,
    pub password_hash: Option<String>,
}

/// The subset of account fields v1's `?field=` parameter accepts.
///
/// Typed rather than stringly so the persistence adapter cannot be handed an arbitrary
/// column name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameField {
    First,
    Last,
}

impl NameField {
    pub fn parse(field: &str) -> Option<Self> {
        match field {
            "firstName" => Some(Self::First),
            "lastName" => Some(Self::Last),
            _ => None,
        }
    }

    /// The JSON key v1 expects the new value under, which matches the query parameter.
    pub fn key(self) -> &'static str {
        match self {
            Self::First => "firstName",
            Self::Last => "lastName",
        }
    }
}

/// A plaintext password that has passed policy checks.
///
/// The 72-byte ceiling is bcrypt's: longer input is silently truncated by most
/// implementations, so it is rejected rather than accepted with a false sense of strength.
#[derive(Clone)]
pub struct Password(String);

impl Password {
    pub const MIN_LEN: usize = 8;
    pub const MAX_BYTES: usize = 72;

    pub fn new(raw: impl Into<String>) -> Result<Self, DomainError> {
        let raw = raw.into();

        if raw.chars().count() < Self::MIN_LEN {
            return Err(DomainError::FieldTooShort {
                field: "Password",
                min: Self::MIN_LEN,
            });
        }
        if raw.len() > Self::MAX_BYTES {
            return Err(DomainError::FieldTooLong {
                field: "Password",
                max: Self::MAX_BYTES,
            });
        }

        Ok(Self(raw))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Keeps the secret out of logs and panic messages.
impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_must_not_be_blank() {
        assert_eq!(
            Account::new("", None, None, false).unwrap_err(),
            DomainError::EmptyField { field: "Login" }
        );
        assert!(Account::new("   ", None, None, false).is_err());
    }

    #[test]
    fn login_is_trimmed() {
        let account = Account::new("  bob  ", None, None, false).unwrap();
        assert_eq!(account.login, "bob");
    }

    #[test]
    fn account_never_serialises_a_password_field() {
        let account = Account::new("bob", None, None, false).unwrap();
        let json = serde_json::to_value(&account).unwrap();
        assert!(json.get("password").is_none());
        assert!(json.get("password_hash").is_none());
    }

    #[test]
    fn password_enforces_minimum_length() {
        assert!(Password::new("1234567").is_err());
        assert!(Password::new("12345678").is_ok());
    }

    #[test]
    fn password_rejects_input_bcrypt_would_truncate() {
        let too_long = "a".repeat(Password::MAX_BYTES + 1);
        assert_eq!(
            Password::new(too_long).unwrap_err(),
            DomainError::FieldTooLong {
                field: "Password",
                max: 72
            }
        );
    }

    #[test]
    fn password_debug_does_not_leak() {
        let password = Password::new("supersecret").unwrap();
        assert_eq!(format!("{password:?}"), "Password(***)");
        assert!(!format!("{password:?}").contains("supersecret"));
    }
}
