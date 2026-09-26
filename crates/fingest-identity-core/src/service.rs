use std::sync::Arc;

use fingest_kernel::{Clock, DomainEvent, EventEnvelope};

use crate::{
    account::{Account, Password, StoredAccount},
    claims::Claims,
    error::IdentityError,
    port::{AccountRepository, PasswordHasher, TokenIssuer, TokenVerifier},
};

/// Self-registration input. Carries no privilege flag: a public route must never be able
/// to create an administrator.
pub struct NewAccount {
    pub login: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub password: String,
}

pub struct AuthService {
    accounts: Arc<dyn AccountRepository>,
    hasher: Arc<dyn PasswordHasher>,
    issuer: Arc<dyn TokenIssuer>,
    verifier: Arc<dyn TokenVerifier>,
    clock: Arc<dyn Clock>,
}

impl AuthService {
    pub fn new(
        accounts: Arc<dyn AccountRepository>,
        hasher: Arc<dyn PasswordHasher>,
        issuer: Arc<dyn TokenIssuer>,
        verifier: Arc<dyn TokenVerifier>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            accounts,
            hasher,
            issuer,
            verifier,
            clock,
        }
    }

    /// Validation precedes the existence check, matching v1's handler-then-service order.
    pub async fn register(&self, new_account: NewAccount) -> Result<Account, IdentityError> {
        let account = Account::new(
            new_account.login,
            new_account.first_name,
            new_account.last_name,
            false,
        )?;
        let password = Password::new(new_account.password)?;

        if self.accounts.exists(&account.login).await? {
            return Err(IdentityError::already_exists(&account.login));
        }

        let hash = self.hasher.hash(&password).await?;
        let events = EventEnvelope::for_all(
            &[DomainEvent::AccountRegistered {
                login: account.login.clone(),
                admin: account.admin,
            }],
            self.clock.now_utc(),
        )?;
        Ok(self.accounts.insert(&account, &hash, &events).await?)
    }

    pub async fn login(
        &self,
        login: &str,
        password: &str,
    ) -> Result<(String, Account), IdentityError> {
        // v1 only required non-empty here; the registration policy must not be applied to
        // existing credentials or it would reject valid legacy passwords.
        if login.is_empty() {
            return Err(IdentityError::Validation(
                "Login cannot be empty".to_owned(),
            ));
        }
        if password.is_empty() {
            return Err(IdentityError::Validation(
                "Password cannot be empty".to_owned(),
            ));
        }

        let Some(StoredAccount {
            account,
            password_hash,
        }) = self.accounts.find(login).await?
        else {
            self.hasher.verify_dummy(password).await?;
            return Err(IdentityError::invalid_credentials());
        };

        let Some(hash) = password_hash else {
            self.hasher.verify_dummy(password).await?;
            return Err(IdentityError::Unauthenticated(
                "User has no password set".to_owned(),
            ));
        };

        if !self.hasher.verify(password, &hash).await? {
            return Err(IdentityError::invalid_credentials());
        }

        let token = self.issuer.issue(&account)?;
        Ok((token, account))
    }

    pub fn verify_token(&self, token: &str) -> Result<Claims, IdentityError> {
        self.verifier
            .verify(token)
            .map_err(|e| IdentityError::Unauthenticated(e.to_string()))
    }

    /// A token outlives the account state it was minted from. Re-reads the account so a
    /// deleted user is rejected and a demoted admin loses admin rights immediately, rather
    /// than when the token expires.
    pub async fn current_principal(&self, mut claims: Claims) -> Result<Claims, IdentityError> {
        let Some(stored) = self.accounts.find(&claims.sub).await? else {
            return Err(IdentityError::Unauthenticated(
                "Account no longer exists".to_owned(),
            ));
        };

        claims.admin = stored.account.admin;
        Ok(claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{CountingHasher, FakeTokens, InMemoryAccountRepository, fixed_clock};
    use futures_executor::block_on;

    fn service(repo: Arc<InMemoryAccountRepository>, hasher: Arc<CountingHasher>) -> AuthService {
        let tokens = Arc::new(FakeTokens);
        AuthService::new(repo, hasher, tokens.clone(), tokens, fixed_clock())
    }

    fn new_account(login: &str, password: &str) -> NewAccount {
        NewAccount {
            login: login.to_owned(),
            first_name: None,
            last_name: None,
            password: password.to_owned(),
        }
    }

    #[test]
    fn register_stores_a_hash_never_the_plaintext() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo.clone(), Arc::new(CountingHasher::new()));

        block_on(svc.register(new_account("bob", "correct-horse"))).unwrap();

        let stored = block_on(repo.find("bob")).unwrap().unwrap();
        let hash = stored.password_hash.unwrap();
        assert_ne!(hash, "correct-horse");
        assert!(hash.starts_with("hashed:"));
        assert_eq!(repo.event_types(), vec!["AccountRegistered"]);
    }

    #[test]
    fn registration_never_creates_an_admin() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo.clone(), Arc::new(CountingHasher::new()));

        let created = block_on(svc.register(new_account("bob", "correct-horse"))).unwrap();

        assert!(!created.admin);
        assert!(!block_on(repo.find("bob")).unwrap().unwrap().account.admin);
    }

    #[test]
    fn register_rejects_a_duplicate_login() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo, Arc::new(CountingHasher::new()));
        block_on(svc.register(new_account("bob", "correct-horse"))).unwrap();

        let err = block_on(svc.register(new_account("bob", "another-one"))).unwrap_err();

        assert_eq!(err, IdentityError::already_exists("bob"));
    }

    #[test]
    fn register_rejects_a_short_password_before_touching_the_repository() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo.clone(), Arc::new(CountingHasher::new()));

        let err = block_on(svc.register(new_account("bob", "short"))).unwrap_err();

        assert!(matches!(err, IdentityError::Validation(_)));
        assert!(repo.is_empty());
    }

    #[test]
    fn login_returns_a_token_for_valid_credentials() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo, Arc::new(CountingHasher::new()));
        block_on(svc.register(new_account("bob", "correct-horse"))).unwrap();

        let (token, account) = block_on(svc.login("bob", "correct-horse")).unwrap();

        assert_eq!(account.login, "bob");
        assert!(token.contains("bob"));
    }

    #[test]
    fn login_with_a_wrong_password_is_rejected() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo, Arc::new(CountingHasher::new()));
        block_on(svc.register(new_account("bob", "correct-horse"))).unwrap();

        let err = block_on(svc.login("bob", "wrong-password")).unwrap_err();

        assert_eq!(err, IdentityError::invalid_credentials());
    }

    #[test]
    fn unknown_and_wrong_password_are_indistinguishable() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo, Arc::new(CountingHasher::new()));
        block_on(svc.register(new_account("bob", "correct-horse"))).unwrap();

        let unknown = block_on(svc.login("nobody", "correct-horse")).unwrap_err();
        let wrong = block_on(svc.login("bob", "wrong-password")).unwrap_err();

        assert_eq!(
            unknown, wrong,
            "response must not reveal which logins exist"
        );
    }

    /// Guards the timing oracle: a miss must still pay the hashing cost.
    #[test]
    fn login_for_an_unknown_account_still_performs_a_verification() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let hasher = Arc::new(CountingHasher::new());
        let svc = service(repo, hasher.clone());

        let _ = block_on(svc.login("nobody", "correct-horse"));

        assert_eq!(
            hasher.verifications(),
            1,
            "a missing account must not short-circuit the hash comparison"
        );
    }

    #[test]
    fn login_rejects_blank_input_without_a_lookup() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo, Arc::new(CountingHasher::new()));

        assert!(matches!(
            block_on(svc.login("", "whatever")).unwrap_err(),
            IdentityError::Validation(_)
        ));
        assert!(matches!(
            block_on(svc.login("bob", "")).unwrap_err(),
            IdentityError::Validation(_)
        ));
    }

    #[test]
    fn account_without_a_password_cannot_log_in() {
        let repo = Arc::new(InMemoryAccountRepository::with_passwordless("legacy"));
        let svc = service(repo, Arc::new(CountingHasher::new()));

        let err = block_on(svc.login("legacy", "anything")).unwrap_err();

        assert_eq!(
            err,
            IdentityError::Unauthenticated("User has no password set".to_owned())
        );
    }

    fn claims_for(login: &str, admin: bool) -> Claims {
        Claims {
            sub: login.to_owned(),
            admin,
            exp: 0,
            iat: 0,
        }
    }

    #[test]
    fn a_token_for_a_deleted_account_is_rejected() {
        let svc = service(
            Arc::new(InMemoryAccountRepository::new()),
            Arc::new(CountingHasher::new()),
        );

        let err = block_on(svc.current_principal(claims_for("ghost", true))).unwrap_err();

        assert_eq!(
            err,
            IdentityError::Unauthenticated("Account no longer exists".to_owned())
        );
    }

    #[test]
    fn a_stale_admin_claim_is_replaced_by_the_stored_flag() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo.clone(), Arc::new(CountingHasher::new()));
        block_on(svc.register(new_account("bob", "correct-horse"))).unwrap();

        let principal = block_on(svc.current_principal(claims_for("bob", true))).unwrap();

        assert!(
            !principal.admin,
            "a demoted admin must not keep admin rights"
        );
        assert_eq!(principal.sub, "bob");
    }

    #[test]
    fn verify_token_maps_failures_to_401() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let svc = service(repo, Arc::new(CountingHasher::new()));

        let err = svc.verify_token("garbage").unwrap_err();

        assert!(matches!(err, IdentityError::Unauthenticated(_)));
        assert!(err.to_string().starts_with("Invalid token:"));
    }
}
