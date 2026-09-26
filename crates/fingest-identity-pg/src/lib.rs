//! Postgres adapter for the identity context.

use async_trait::async_trait;
use fingest_identity_core::{Account, AccountRepository, NameField, StoredAccount};
use fingest_kernel::{EventEnvelope, PortError};
use sqlx::{PgPool, Postgres, Transaction};

pub struct PgAccountRepository {
    pool: PgPool,
}

impl PgAccountRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn to_port_error(err: sqlx::Error) -> PortError {
    if let sqlx::Error::Database(ref db_err) = err
        && db_err.code().as_deref() == Some("23505")
    {
        return PortError::Conflict("Account already exists".to_owned());
    }
    PortError::Storage(err.to_string())
}

async fn append_events(
    tx: &mut Transaction<'_, Postgres>,
    events: &[EventEnvelope],
) -> Result<(), PortError> {
    for envelope in events {
        sqlx::query!(
            r#"INSERT INTO outbox (aggregate, event_type, payload, occurred_at)
               VALUES ($1, $2, $3, $4)"#,
            envelope.aggregate.as_str(),
            envelope.event_type.as_str(),
            envelope.payload,
            envelope.occurred_at
        )
        .execute(&mut **tx)
        .await
        .map_err(to_port_error)?;
    }
    Ok(())
}

#[async_trait]
impl AccountRepository for PgAccountRepository {
    async fn find(&self, login: &str) -> Result<Option<StoredAccount>, PortError> {
        let row = sqlx::query!(
            r#"SELECT login, first_name, last_name, password, admin
               FROM account WHERE login = $1"#,
            login
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_port_error)?;

        // Built directly rather than through `Account::new`: the row is already persisted,
        // and a validation change must not make existing accounts unreadable.
        Ok(row.map(|row| StoredAccount {
            account: Account {
                login: row.login,
                first_name: row.first_name,
                last_name: row.last_name,
                admin: row.admin,
            },
            password_hash: row.password,
        }))
    }

    async fn exists(&self, login: &str) -> Result<bool, PortError> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM account WHERE login = $1) AS "exists!""#,
            login
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn insert(
        &self,
        account: &Account,
        password_hash: &str,
        events: &[EventEnvelope],
    ) -> Result<Account, PortError> {
        let mut tx = self.pool.begin().await.map_err(to_port_error)?;

        let row = sqlx::query!(
            r#"INSERT INTO account (login, first_name, last_name, password, admin)
               VALUES ($1, $2, $3, $4, $5)
               RETURNING login, first_name, last_name, admin"#,
            account.login,
            account.first_name,
            account.last_name,
            password_hash,
            account.admin
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(to_port_error)?;

        append_events(&mut tx, events).await?;
        tx.commit().await.map_err(to_port_error)?;

        Ok(Account {
            login: row.login,
            first_name: row.first_name,
            last_name: row.last_name,
            admin: row.admin,
        })
    }

    async fn list(&self) -> Result<Vec<Account>, PortError> {
        let rows = sqlx::query!(
            r#"SELECT login, first_name, last_name, admin FROM account ORDER BY login"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_port_error)?;

        Ok(rows
            .into_iter()
            .map(|row| Account {
                login: row.login,
                first_name: row.first_name,
                last_name: row.last_name,
                admin: row.admin,
            })
            .collect())
    }

    /// One static query per field rather than an interpolated column name, so the update
    /// path has no way to reach an arbitrary column.
    async fn update_name(
        &self,
        login: &str,
        field: NameField,
        value: &str,
    ) -> Result<u64, PortError> {
        let result = match field {
            NameField::First => {
                sqlx::query!(
                    r#"UPDATE account SET first_name = $1 WHERE login = $2"#,
                    value,
                    login
                )
                .execute(&self.pool)
                .await
            }
            NameField::Last => {
                sqlx::query!(
                    r#"UPDATE account SET last_name = $1 WHERE login = $2"#,
                    value,
                    login
                )
                .execute(&self.pool)
                .await
            }
        };

        result.map(|r| r.rows_affected()).map_err(to_port_error)
    }

    async fn delete(&self, login: &str, events: &[EventEnvelope]) -> Result<u64, PortError> {
        let mut tx = self.pool.begin().await.map_err(to_port_error)?;

        let deleted = sqlx::query!(r#"DELETE FROM account WHERE login = $1"#, login)
            .execute(&mut *tx)
            .await
            .map(|r| r.rows_affected())
            .map_err(to_port_error)?;

        if deleted > 0 {
            append_events(&mut tx, events).await?;
            tx.commit().await.map_err(to_port_error)?;
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(login: &str, admin: bool) -> Account {
        Account::new(login, Some("Test".into()), Some("User".into()), admin).expect("valid fixture")
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn insert_then_find_round_trips(pool: PgPool) {
        let repo = PgAccountRepository::new(pool);

        repo.insert(&account("zoe", false), "$2b$12$fakehashvalue", &[])
            .await
            .unwrap();

        let found = repo.find("zoe").await.unwrap().unwrap();
        assert_eq!(found.account.login, "zoe");
        assert_eq!(found.account.first_name.as_deref(), Some("Test"));
        assert!(!found.account.admin);
        assert_eq!(found.password_hash.as_deref(), Some("$2b$12$fakehashvalue"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn find_returns_none_for_unknown_login(pool: PgPool) {
        assert!(
            PgAccountRepository::new(pool)
                .find("nobody")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn exists_reflects_the_row(pool: PgPool) {
        let repo = PgAccountRepository::new(pool);

        assert!(!repo.exists("zoe").await.unwrap());
        repo.insert(&account("zoe", false), "hash", &[])
            .await
            .unwrap();
        assert!(repo.exists("zoe").await.unwrap());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn duplicate_login_maps_to_conflict(pool: PgPool) {
        let repo = PgAccountRepository::new(pool);
        repo.insert(&account("zoe", false), "hash", &[])
            .await
            .unwrap();

        let err = repo
            .insert(&account("zoe", false), "hash", &[])
            .await
            .unwrap_err();

        assert!(matches!(err, PortError::Conflict(_)), "got {err:?}");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn admin_flag_persists(pool: PgPool) {
        let repo = PgAccountRepository::new(pool);
        repo.insert(&account("root", true), "hash", &[])
            .await
            .unwrap();

        assert!(repo.find("root").await.unwrap().unwrap().account.admin);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_includes_the_seeded_accounts(pool: PgPool) {
        let logins: Vec<String> = PgAccountRepository::new(pool)
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|a| a.login)
            .collect();

        assert!(logins.contains(&"admin".to_owned()));
        assert!(logins.contains(&"user1".to_owned()));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_name_touches_only_the_named_field(pool: PgPool) {
        let repo = PgAccountRepository::new(pool);
        repo.insert(&account("zoe", false), "hash", &[])
            .await
            .unwrap();

        assert_eq!(
            repo.update_name("zoe", NameField::First, "Zoraida")
                .await
                .unwrap(),
            1
        );

        let stored = repo.find("zoe").await.unwrap().unwrap();
        assert_eq!(stored.account.first_name.as_deref(), Some("Zoraida"));
        assert_eq!(stored.account.last_name.as_deref(), Some("User"));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_name_reports_zero_for_a_missing_account(pool: PgPool) {
        assert_eq!(
            PgAccountRepository::new(pool)
                .update_name("ghost", NameField::Last, "X")
                .await
                .unwrap(),
            0
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn delete_reports_rows_affected(pool: PgPool) {
        let repo = PgAccountRepository::new(pool);
        repo.insert(&account("zoe", false), "hash", &[])
            .await
            .unwrap();

        assert_eq!(repo.delete("zoe", &[]).await.unwrap(), 1);
        assert_eq!(repo.delete("zoe", &[]).await.unwrap(), 0);
    }

    /// `account_wallet`, `budget` and `saving` all cascade from `account`.
    #[sqlx::test(migrations = "../../migrations")]
    async fn deleting_an_account_cascades_to_its_budgets(pool: PgPool) {
        sqlx::query!(
            r#"INSERT INTO budget
               (account_login, category_name, category_profit, total_amount, total_currency,
                start_date, end_date)
               VALUES ('user1', 'Food', false, 100.00, 'USD', DATE '2024-01-01', DATE '2024-12-31')"#
        )
        .execute(&pool)
        .await
        .unwrap();

        PgAccountRepository::new(pool.clone())
            .delete("user1", &[])
            .await
            .unwrap();

        let remaining = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM budget WHERE account_login = 'user1'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(remaining, 0);
    }
}
