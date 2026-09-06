//! bcrypt and jsonwebtoken adapters.
//!
//! Algorithm choices are pinned to v1 (bcrypt cost 12, HS256, `Validation::default()`) so
//! tokens and hashes remain interchangeable between versions.

use std::sync::Arc;

use chrono::Duration;
use fingest_identity_core::{
    Account, Claims, Password, PasswordHasher, TokenError, TokenIssuer, TokenVerifier,
};
use fingest_kernel::{Clock, PortError};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};

pub struct BcryptHasher {
    cost: u32,
    /// Precomputed so a login for a missing account can pay the same cost as a real one.
    dummy_hash: String,
}

impl BcryptHasher {
    pub const DEFAULT_COST: u32 = bcrypt::DEFAULT_COST;

    pub fn new(cost: u32) -> Result<Self, PortError> {
        let dummy_hash = bcrypt::hash("dummy-password-for-timing-parity", cost)
            .map_err(|e| PortError::Encoding(e.to_string()))?;
        Ok(Self { cost, dummy_hash })
    }
}

impl PasswordHasher for BcryptHasher {
    fn hash(&self, password: &Password) -> Result<String, PortError> {
        bcrypt::hash(password.expose(), self.cost).map_err(|e| PortError::Encoding(e.to_string()))
    }

    fn verify(&self, password: &str, hash: &str) -> Result<bool, PortError> {
        match bcrypt::verify(password, hash) {
            Ok(valid) => Ok(valid),
            // A stored value that is not a bcrypt hash — v1 seeded plaintext passwords —
            // is a failed authentication, not a server error.
            Err(_) => Ok(false),
        }
    }

    fn verify_dummy(&self, password: &str) -> Result<(), PortError> {
        let _ = bcrypt::verify(password, &self.dummy_hash);
        Ok(())
    }
}

pub struct JwtTokens {
    encoding: EncodingKey,
    decoding: DecodingKey,
    expiration_hours: i64,
    clock: Arc<dyn Clock>,
}

impl JwtTokens {
    pub fn new(secret: &str, expiration_hours: i64, clock: Arc<dyn Clock>) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            expiration_hours,
            clock,
        }
    }
}

impl TokenIssuer for JwtTokens {
    fn issue(&self, account: &Account) -> Result<String, PortError> {
        let now = self.clock.now_utc();
        let claims = Claims {
            sub: account.login.clone(),
            admin: account.admin,
            exp: (now + Duration::hours(self.expiration_hours)).timestamp(),
            iat: now.timestamp(),
        };

        encode(&Header::default(), &claims, &self.encoding)
            .map_err(|e| PortError::Encoding(e.to_string()))
    }
}

impl TokenVerifier for JwtTokens {
    fn verify(&self, token: &str) -> Result<Claims, TokenError> {
        decode::<Claims>(token, &self.decoding, &Validation::default())
            .map(|data| data.claims)
            .map_err(|e| TokenError::Invalid(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use fingest_kernel::FixedClock;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    fn now() -> i64 {
        Utc::now().timestamp()
    }

    fn account(login: &str, admin: bool) -> Account {
        Account::new(login, None, None, admin).expect("valid fixture")
    }

    fn clock_at(ts: i64) -> Arc<dyn Clock> {
        Arc::new(FixedClock(Utc.timestamp_opt(ts, 0).unwrap()))
    }

    fn tokens(now: i64) -> JwtTokens {
        JwtTokens::new(SECRET, 24, clock_at(now))
    }

    // --- hashing ---

    #[test]
    fn hash_is_not_the_plaintext_and_verifies() {
        let hasher = BcryptHasher::new(4).unwrap(); // low cost keeps the test fast
        let password = Password::new("correct-horse").unwrap();

        let hash = hasher.hash(&password).unwrap();

        assert_ne!(hash, "correct-horse");
        assert!(hasher.verify("correct-horse", &hash).unwrap());
        assert!(!hasher.verify("wrong-horse", &hash).unwrap());
    }

    #[test]
    fn hashes_are_salted_so_two_runs_differ() {
        let hasher = BcryptHasher::new(4).unwrap();
        let password = Password::new("correct-horse").unwrap();

        assert_ne!(
            hasher.hash(&password).unwrap(),
            hasher.hash(&password).unwrap()
        );
    }

    /// v1 seeded plaintext passwords; verifying against one must fail closed, not 500.
    #[test]
    fn a_non_bcrypt_stored_value_fails_authentication() {
        let hasher = BcryptHasher::new(4).unwrap();

        assert!(!hasher.verify("password123", "password123").unwrap());
    }

    #[test]
    fn dummy_verification_succeeds_without_revealing_anything() {
        let hasher = BcryptHasher::new(4).unwrap();
        assert!(hasher.verify_dummy("anything").is_ok());
    }

    // --- tokens ---

    #[test]
    fn issued_token_round_trips() {
        let issued_at = now();
        let tokens = tokens(issued_at);

        let token = tokens.issue(&account("bob", true)).unwrap();
        let claims = tokens.verify(&token).unwrap();

        assert_eq!(claims.sub, "bob");
        assert!(claims.admin);
        assert_eq!(claims.iat, issued_at);
        assert_eq!(claims.exp, issued_at + 24 * 3600);
    }

    #[test]
    fn a_token_signed_with_another_secret_is_rejected() {
        let token = tokens(now()).issue(&account("bob", false)).unwrap();
        let other = JwtTokens::new("ffffffffffffffffffffffffffffffff", 24, clock_at(now()));

        assert!(matches!(
            other.verify(&token).unwrap_err(),
            TokenError::Invalid(_)
        ));
    }

    /// `Validation::default()` checks `exp` against the real wall clock, so a token minted
    /// with a 2023 issue time and a 24h lifetime is already long expired.
    #[test]
    fn an_expired_token_is_rejected() {
        let tokens = tokens(1_700_000_000);
        let token = tokens.issue(&account("bob", false)).unwrap();

        assert!(matches!(
            tokens.verify(&token).unwrap_err(),
            TokenError::Invalid(_)
        ));
    }

    #[test]
    fn a_currently_valid_token_is_accepted() {
        let tokens = tokens(now());
        let token = tokens.issue(&account("bob", false)).unwrap();

        assert_eq!(tokens.verify(&token).unwrap().sub, "bob");
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(tokens(now()).verify("not-a-jwt").is_err());
    }

    #[test]
    fn admin_flag_survives_the_round_trip() {
        let tokens = tokens(now());
        let token = tokens.issue(&account("alice", false)).unwrap();

        assert!(!tokens.verify(&token).unwrap().admin);
    }
}
