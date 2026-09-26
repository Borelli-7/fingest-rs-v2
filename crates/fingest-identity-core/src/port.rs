use async_trait::async_trait;
use fingest_kernel::PortError;
use thiserror::Error;

use crate::{
    account::{Account, NameField, Password, StoredAccount},
    claims::Claims,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TokenError {
    /// v1's wording: `"Invalid token: {source}"`.
    #[error("Invalid token: {0}")]
    Invalid(String),
}

#[async_trait]
pub trait AccountRepository: Send + Sync {
    async fn find(&self, login: &str) -> Result<Option<StoredAccount>, PortError>;

    async fn exists(&self, login: &str) -> Result<bool, PortError>;

    async fn insert(&self, account: &Account, password_hash: &str) -> Result<Account, PortError>;

    async fn list(&self) -> Result<Vec<Account>, PortError>;

    /// Returns the number of rows updated, so a missing account is distinguishable from a
    /// successful write without a second round trip.
    async fn update_name(
        &self,
        login: &str,
        field: NameField,
        value: &str,
    ) -> Result<u64, PortError>;

    async fn delete(&self, login: &str) -> Result<u64, PortError>;
}

/// Async so an adapter can move CPU-bound hashing off the request executor; a synchronous
/// bcrypt call inside a handler stalls every other request on that worker.
#[async_trait]
pub trait PasswordHasher: Send + Sync {
    async fn hash(&self, password: &Password) -> Result<String, PortError>;

    async fn verify(&self, password: &str, hash: &str) -> Result<bool, PortError>;

    /// Runs a verification against a fixed hash so that a login for a non-existent account
    /// costs the same as one with a wrong password. Without this, response latency reveals
    /// which logins exist.
    async fn verify_dummy(&self, password: &str) -> Result<(), PortError>;
}

pub trait TokenIssuer: Send + Sync {
    fn issue(&self, account: &Account) -> Result<String, PortError>;
}

pub trait TokenVerifier: Send + Sync {
    fn verify(&self, token: &str) -> Result<Claims, TokenError>;
}
