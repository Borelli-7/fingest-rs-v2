//! bcrypt and jsonwebtoken adapters.
//!
//! Algorithm choices are pinned to v1 (bcrypt cost 12, HS256, `Validation::default()`) so
//! tokens and hashes remain interchangeable between versions.

use std::sync::Arc;

use async_trait::async_trait;
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

/// bcrypt costs hundreds of milliseconds of CPU; run it on the blocking pool so it never
/// occupies an actix worker thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, PortError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| PortError::Unavailable(format!("password hashing task failed: {e}")))
}

#[async_trait]
impl PasswordHasher for BcryptHasher {
    async fn hash(&self, password: &Password) -> Result<String, PortError> {
        let (password, cost) = (password.expose().to_owned(), self.cost);
        blocking(move || bcrypt::hash(password, cost))
            .await?
            .map_err(|e| PortError::Encoding(e.to_string()))
    }

    async fn verify(&self, password: &str, hash: &str) -> Result<bool, PortError> {
        let (password, hash) = (password.to_owned(), hash.to_owned());
        // A stored value that is not a bcrypt hash — v1 seeded plaintext passwords —
        // is a failed authentication, not a server error.
        blocking(move || bcrypt::verify(password, &hash).unwrap_or(false)).await
    }

    async fn verify_dummy(&self, password: &str) -> Result<(), PortError> {
        let (password, hash) = (password.to_owned(), self.dummy_hash.clone());
        blocking(move || {
            let _ = bcrypt::verify(password, &hash);
        })
        .await
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

    #[tokio::test]
    async fn hash_is_not_the_plaintext_and_verifies() {
        let hasher = BcryptHasher::new(4).unwrap(); // low cost keeps the test fast
        let password = Password::new("correct-horse").unwrap();

        let hash = hasher.hash(&password).await.unwrap();

        assert_ne!(hash, "correct-horse");
        assert!(hasher.verify("correct-horse", &hash).await.unwrap());
        assert!(!hasher.verify("wrong-horse", &hash).await.unwrap());
    }

    #[tokio::test]
    async fn hashes_are_salted_so_two_runs_differ() {
        let hasher = BcryptHasher::new(4).unwrap();
        let password = Password::new("correct-horse").unwrap();

        assert_ne!(
            hasher.hash(&password).await.unwrap(),
            hasher.hash(&password).await.unwrap()
        );
    }

    /// v1 seeded plaintext passwords; verifying against one must fail closed, not 500.
    #[tokio::test]
    async fn a_non_bcrypt_stored_value_fails_authentication() {
        let hasher = BcryptHasher::new(4).unwrap();

        assert!(!hasher.verify("password123", "password123").await.unwrap());
    }

    #[tokio::test]
    async fn dummy_verification_succeeds_without_revealing_anything() {
        let hasher = BcryptHasher::new(4).unwrap();
        assert!(hasher.verify_dummy("anything").await.is_ok());
    }

    /// `#[tokio::test]` is single-threaded: if bcrypt ran inline, the ticker could not
    /// advance until the hash finished.
    #[tokio::test]
    async fn hashing_does_not_block_the_executor() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let hasher = BcryptHasher::new(10).unwrap();
        let password = Password::new("correct-horse").unwrap();
        let ticks = Arc::new(AtomicUsize::new(0));
        let ticker = {
            let ticks = Arc::clone(&ticks);
            tokio::spawn(async move {
                loop {
                    ticks.fetch_add(1, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                }
            })
        };

        hasher.hash(&password).await.unwrap();
        let observed = ticks.load(Ordering::SeqCst);
        ticker.abort();

        assert!(observed > 0, "other tasks must run while bcrypt works");
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
