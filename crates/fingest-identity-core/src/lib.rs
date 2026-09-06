//! Identity bounded context.
//!
//! Owns accounts, credential policy, JWT claims and the authentication use cases. Knows
//! nothing about bcrypt, jsonwebtoken, Postgres or HTTP — those arrive as port impls.

pub mod account;
pub mod claims;
pub mod error;
pub mod port;
pub mod service;
pub mod user_service;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use account::{Account, NameField, Password, StoredAccount};
pub use claims::Claims;
pub use error::IdentityError;
pub use port::{AccountRepository, PasswordHasher, TokenError, TokenIssuer, TokenVerifier};
pub use service::{AuthService, NewAccount};
pub use user_service::UserService;
