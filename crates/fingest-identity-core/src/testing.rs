//! In-memory doubles for identity use-case tests.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use fingest_kernel::{Clock, EventEnvelope, FixedClock, PortError};

use crate::{
    account::{Account, NameField, Password, StoredAccount},
    claims::Claims,
    port::{AccountRepository, PasswordHasher, TokenError, TokenIssuer, TokenVerifier},
};

/// A clock pinned to an arbitrary instant, so event timestamps are deterministic.
pub fn fixed_clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock(
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid fixture timestamp"),
    ))
}

#[derive(Default)]
pub struct InMemoryAccountRepository {
    rows: Mutex<Vec<StoredAccount>>,
    events: Mutex<Vec<EventEnvelope>>,
}

impl InMemoryAccountRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mirrors v1's seeded rows, which have a NULL-equivalent password.
    pub fn with_passwordless(login: &str) -> Self {
        Self {
            rows: Mutex::new(vec![StoredAccount {
                account: Account::new(login, None, None, false).expect("valid fixture"),
                password_hash: None,
            }]),
            events: Mutex::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.lock().expect("lock poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn event_types(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("lock poisoned")
            .iter()
            .map(|e| e.event_type.clone())
            .collect()
    }
}

#[async_trait]
impl AccountRepository for InMemoryAccountRepository {
    async fn find(&self, login: &str) -> Result<Option<StoredAccount>, PortError> {
        Ok(self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|row| row.account.login == login)
            .cloned())
    }

    async fn exists(&self, login: &str) -> Result<bool, PortError> {
        Ok(self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|row| row.account.login == login))
    }

    async fn insert(
        &self,
        account: &Account,
        password_hash: &str,
        events: &[EventEnvelope],
    ) -> Result<Account, PortError> {
        self.rows
            .lock()
            .expect("lock poisoned")
            .push(StoredAccount {
                account: account.clone(),
                password_hash: Some(password_hash.to_owned()),
            });
        self.events
            .lock()
            .expect("lock poisoned")
            .extend_from_slice(events);
        Ok(account.clone())
    }

    async fn list(&self) -> Result<Vec<Account>, PortError> {
        Ok(self
            .rows
            .lock()
            .expect("lock poisoned")
            .iter()
            .map(|row| row.account.clone())
            .collect())
    }

    async fn update_name(
        &self,
        login: &str,
        field: NameField,
        value: &str,
    ) -> Result<u64, PortError> {
        let mut rows = self.rows.lock().expect("lock poisoned");
        let Some(row) = rows.iter_mut().find(|row| row.account.login == login) else {
            return Ok(0);
        };

        match field {
            NameField::First => row.account.first_name = Some(value.to_owned()),
            NameField::Last => row.account.last_name = Some(value.to_owned()),
        }
        Ok(1)
    }

    async fn delete(&self, login: &str, events: &[EventEnvelope]) -> Result<u64, PortError> {
        let mut rows = self.rows.lock().expect("lock poisoned");
        let before = rows.len();
        rows.retain(|row| row.account.login != login);
        let deleted = (before - rows.len()) as u64;
        if deleted > 0 {
            self.events
                .lock()
                .expect("lock poisoned")
                .extend_from_slice(events);
        }
        Ok(deleted)
    }
}

/// Reversible stand-in for bcrypt that also records how often it was asked to verify,
/// so tests can assert the timing-oracle guard actually runs.
#[derive(Default)]
pub struct CountingHasher {
    verifications: AtomicUsize,
}

impl CountingHasher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn verifications(&self) -> usize {
        self.verifications.load(Ordering::SeqCst)
    }
}

impl PasswordHasher for CountingHasher {
    fn hash(&self, password: &Password) -> Result<String, PortError> {
        Ok(format!("hashed:{}", password.expose()))
    }

    fn verify(&self, password: &str, hash: &str) -> Result<bool, PortError> {
        self.verifications.fetch_add(1, Ordering::SeqCst);
        Ok(hash == format!("hashed:{password}"))
    }

    fn verify_dummy(&self, password: &str) -> Result<(), PortError> {
        self.verify(password, "hashed:$never$")?;
        Ok(())
    }
}

/// Issues and accepts a trivially decodable token.
pub struct FakeTokens;

impl TokenIssuer for FakeTokens {
    fn issue(&self, account: &Account) -> Result<String, PortError> {
        Ok(format!(
            "token-for-{}-admin={}",
            account.login, account.admin
        ))
    }
}

impl TokenVerifier for FakeTokens {
    fn verify(&self, token: &str) -> Result<Claims, TokenError> {
        let rest = token
            .strip_prefix("token-for-")
            .ok_or_else(|| TokenError::Invalid("malformed".to_owned()))?;
        let (login, admin) = rest
            .split_once("-admin=")
            .ok_or_else(|| TokenError::Invalid("malformed".to_owned()))?;

        Ok(Claims {
            sub: login.to_owned(),
            admin: admin == "true",
            exp: 1_900_000_000,
            iat: 1_700_000_000,
        })
    }
}
