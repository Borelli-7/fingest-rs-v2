use std::sync::Arc;

use fingest_kernel::{Clock, DomainEvent, EventEnvelope};

use crate::{
    account::{Account, NameField},
    error::IdentityError,
    port::AccountRepository,
};

/// Account administration use cases behind `/resources/users`.
///
/// Authorization is enforced at the HTTP boundary, which is where the caller's identity
/// lives; this layer only knows about accounts.
pub struct UserService {
    accounts: Arc<dyn AccountRepository>,
    clock: Arc<dyn Clock>,
}

impl UserService {
    pub fn new(accounts: Arc<dyn AccountRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { accounts, clock }
    }

    pub async fn list(&self) -> Result<Vec<Account>, IdentityError> {
        Ok(self.accounts.list().await?)
    }

    pub async fn update_name(
        &self,
        login: &str,
        field: &str,
        value: Option<&str>,
    ) -> Result<(), IdentityError> {
        let field = NameField::parse(field)
            .ok_or_else(|| IdentityError::BadRequest(format!("Invalid field: {field}")))?;

        let value = value.ok_or_else(|| {
            IdentityError::BadRequest(format!("{} value is required", field.key()))
        })?;

        if self.accounts.update_name(login, field, value).await? == 0 {
            return Err(IdentityError::NotFound(format!(
                "user does not exist: {login}"
            )));
        }

        Ok(())
    }

    pub async fn delete(&self, login: &str) -> Result<(), IdentityError> {
        if !self.accounts.exists(login).await? {
            return Err(IdentityError::NotFound(format!(
                "User with login '{login}' not found"
            )));
        }

        let events = EventEnvelope::for_all(
            &[DomainEvent::AccountDeleted {
                login: login.to_owned(),
            }],
            self.clock.now_utc(),
        )?;
        if self.accounts.delete(login, &events).await? == 0 {
            // Row vanished between the check and the delete.
            return Err(IdentityError::Internal("Failed to delete user".to_owned()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{InMemoryAccountRepository, fixed_clock};
    use futures_executor::block_on;

    fn service(repo: Arc<InMemoryAccountRepository>) -> UserService {
        UserService::new(repo, fixed_clock())
    }

    fn seeded() -> Arc<InMemoryAccountRepository> {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let account = Account::new("bob", Some("Bob".into()), Some("Builder".into()), false)
            .expect("valid fixture");
        block_on(repo.insert(&account, "hash", &[])).unwrap();
        repo
    }

    #[test]
    fn list_returns_every_account() {
        assert_eq!(block_on(service(seeded()).list()).unwrap().len(), 1);
    }

    #[test]
    fn update_first_name() {
        let repo = seeded();
        let svc = service(repo.clone());

        block_on(svc.update_name("bob", "firstName", Some("Robert"))).unwrap();

        let stored = block_on(repo.find("bob")).unwrap().unwrap();
        assert_eq!(stored.account.first_name.as_deref(), Some("Robert"));
        assert_eq!(
            stored.account.last_name.as_deref(),
            Some("Builder"),
            "the other field must be untouched"
        );
    }

    #[test]
    fn update_last_name() {
        let repo = seeded();
        let svc = service(repo.clone());

        block_on(svc.update_name("bob", "lastName", Some("Fixer"))).unwrap();

        let stored = block_on(repo.find("bob")).unwrap().unwrap();
        assert_eq!(stored.account.last_name.as_deref(), Some("Fixer"));
    }

    #[test]
    fn unknown_field_is_rejected_with_v1_wording() {
        let err =
            block_on(service(seeded()).update_name("bob", "admin", Some("true"))).unwrap_err();

        assert_eq!(
            err,
            IdentityError::BadRequest("Invalid field: admin".into())
        );
    }

    /// A writable `admin` field would be a privilege-escalation path.
    #[test]
    fn the_admin_flag_is_not_updatable() {
        assert!(NameField::parse("admin").is_none());
        assert!(NameField::parse("password").is_none());
    }

    #[test]
    fn a_missing_value_is_rejected() {
        let err = block_on(service(seeded()).update_name("bob", "firstName", None)).unwrap_err();

        assert_eq!(
            err,
            IdentityError::BadRequest("firstName value is required".into())
        );
    }

    #[test]
    fn updating_an_unknown_account_is_404() {
        let err =
            block_on(service(seeded()).update_name("ghost", "firstName", Some("X"))).unwrap_err();

        assert_eq!(
            err,
            IdentityError::NotFound("user does not exist: ghost".into())
        );
    }

    #[test]
    fn delete_removes_the_account() {
        let repo = seeded();
        block_on(service(repo.clone()).delete("bob")).unwrap();

        assert!(repo.is_empty());
        assert_eq!(repo.event_types(), vec!["AccountDeleted"]);
    }

    #[test]
    fn deleting_an_unknown_account_is_404() {
        let err = block_on(service(seeded()).delete("ghost")).unwrap_err();

        assert_eq!(
            err,
            IdentityError::NotFound("User with login 'ghost' not found".into())
        );
    }
}
