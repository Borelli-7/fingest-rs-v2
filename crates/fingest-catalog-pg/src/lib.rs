//! Postgres adapter for the catalog context.
//!
//! Queries use the `query_as!` macros so the SQL is verified against the real schema at
//! compile time — v1 used only runtime `sqlx::query()`, so its `sqlx-data.json` and
//! `prepare_sqlx.sh` checked nothing.

use async_trait::async_trait;
use fingest_catalog_core::{Category, CategoryRepository};
use fingest_kernel::PortError;
use sqlx::PgPool;

pub struct PgCategoryRepository {
    pool: PgPool,
}

impl PgCategoryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Translates driver failures into port vocabulary.
///
/// Deviation D11: v1 let constraint violations fall through as `DatabaseError` → 500. A
/// duplicate or an in-use category is a client-correctable conflict, so both become 409.
fn to_port_error(err: sqlx::Error) -> PortError {
    if let sqlx::Error::Database(ref db_err) = err {
        match db_err.code().as_deref() {
            Some("23505") => {
                return PortError::Conflict("Category already exists".to_owned());
            }
            Some("23503") => {
                return PortError::Conflict(
                    "Category is referenced by existing expenses or budgets".to_owned(),
                );
            }
            _ => {}
        }
    }
    PortError::Storage(err.to_string())
}

#[async_trait]
impl CategoryRepository for PgCategoryRepository {
    async fn list(&self) -> Result<Vec<Category>, PortError> {
        // v1 left the order unspecified; ordering makes responses reproducible.
        sqlx::query_as!(
            Category,
            r#"SELECT name, profit FROM category ORDER BY name, profit"#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn find(&self, name: &str, profit: bool) -> Result<Option<Category>, PortError> {
        sqlx::query_as!(
            Category,
            r#"SELECT name, profit FROM category WHERE name = $1 AND profit = $2"#,
            name,
            profit
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn find_conflict(
        &self,
        name: &str,
        profit: bool,
        excluding: Option<&str>,
    ) -> Result<Option<Category>, PortError> {
        sqlx::query_as!(
            Category,
            r#"
            SELECT name, profit FROM category
            WHERE LOWER(name) = LOWER($1)
              AND profit = $2
              AND ($3::text IS NULL OR name <> $3)
            LIMIT 1
            "#,
            name,
            profit,
            excluding
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn insert(&self, category: &Category) -> Result<Category, PortError> {
        sqlx::query_as!(
            Category,
            r#"INSERT INTO category (name, profit) VALUES ($1, $2) RETURNING name, profit"#,
            category.name,
            category.profit
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn rename(
        &self,
        name: &str,
        profit: bool,
        new_name: &str,
    ) -> Result<Category, PortError> {
        sqlx::query_as!(
            Category,
            r#"
            UPDATE category SET name = $1
            WHERE name = $2 AND profit = $3
            RETURNING name, profit
            "#,
            new_name,
            name,
            profit
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn delete(&self, name: &str, profit: bool) -> Result<u64, PortError> {
        sqlx::query!(
            r#"DELETE FROM category WHERE name = $1 AND profit = $2"#,
            name,
            profit
        )
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected())
        .map_err(to_port_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#[sqlx::test]` provisions a fresh database per test and applies every migration,
    /// so the v1 sample data is present. Tests use names outside that seed set.
    const MIGRATIONS: &str = "../../migrations";

    fn repo(pool: PgPool) -> PgCategoryRepository {
        PgCategoryRepository::new(pool)
    }

    fn category(name: &str, profit: bool) -> Category {
        Category::new(name, profit).expect("valid fixture")
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_includes_seeded_categories(pool: PgPool) {
        let found = repo(pool).list().await.unwrap();

        assert!(found.contains(&category("Food", false)));
        assert!(found.contains(&category("Salary", true)));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn list_is_ordered(pool: PgPool) {
        let found = repo(pool).list().await.unwrap();

        let mut sorted = found.clone();
        sorted.sort_by(|a, b| (&a.name, a.profit).cmp(&(&b.name, b.profit)));
        assert_eq!(found, sorted);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn insert_then_find_round_trips(pool: PgPool) {
        let repo = repo(pool);
        let inserted = repo.insert(&category("Zebra", false)).await.unwrap();

        assert_eq!(inserted, category("Zebra", false));
        assert_eq!(
            repo.find("Zebra", false).await.unwrap(),
            Some(category("Zebra", false))
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn find_is_sensitive_to_the_profit_flag(pool: PgPool) {
        let repo = repo(pool);
        repo.insert(&category("Zebra", false)).await.unwrap();

        assert!(repo.find("Zebra", true).await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn duplicate_insert_maps_to_conflict(pool: PgPool) {
        let repo = repo(pool);
        repo.insert(&category("Zebra", false)).await.unwrap();

        let err = repo.insert(&category("Zebra", false)).await.unwrap_err();

        assert!(
            matches!(err, PortError::Conflict(_)),
            "unique violation must not surface as a 500, got {err:?}"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn find_conflict_ignores_case(pool: PgPool) {
        let found = repo(pool).find_conflict("fOoD", false, None).await.unwrap();

        assert_eq!(found, Some(category("Food", false)));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn find_conflict_excludes_the_named_row(pool: PgPool) {
        let found = repo(pool)
            .find_conflict("Food", false, Some("Food"))
            .await
            .unwrap();

        assert_eq!(found, None, "a rename must not conflict with itself");
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn rename_updates_the_row(pool: PgPool) {
        let repo = repo(pool);
        repo.insert(&category("Zebra", false)).await.unwrap();

        let renamed = repo.rename("Zebra", false, "Yak").await.unwrap();

        assert_eq!(renamed.name, "Yak");
        assert!(repo.find("Zebra", false).await.unwrap().is_none());
        assert!(repo.find("Yak", false).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn delete_reports_rows_affected(pool: PgPool) {
        let repo = repo(pool);
        repo.insert(&category("Zebra", false)).await.unwrap();

        assert_eq!(repo.delete("Zebra", false).await.unwrap(), 1);
        assert_eq!(repo.delete("Zebra", false).await.unwrap(), 0);
    }

    /// The composite FK `(category_name, category_profit)` has no ON DELETE action, so a
    /// referenced category cannot be removed.
    #[sqlx::test(migrations = "../../migrations")]
    async fn deleting_a_referenced_category_maps_to_conflict(pool: PgPool) {
        sqlx::query!(
            r#"INSERT INTO wallet (id, name, amount_amount, amount_currency)
               VALUES (9001, 'Test Wallet', 100.00, 'USD')"#
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query!(
            r#"INSERT INTO expense
               (wallet_id, amount_amount, amount_currency, date, description,
                category_name, category_profit)
               VALUES (9001, 10.00, 'USD', DATE '2024-01-01', 'lunch', 'Food', false)"#
        )
        .execute(&pool)
        .await
        .unwrap();

        let err = repo(pool).delete("Food", false).await.unwrap_err();

        assert!(
            matches!(err, PortError::Conflict(_)),
            "FK violation must surface as 409, got {err:?}"
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn renaming_a_referenced_category_carries_its_references_along(pool: PgPool) {
        sqlx::query!(
            r#"INSERT INTO wallet (id, name, amount_amount, amount_currency)
               VALUES (9002, 'Test Wallet', 100.00, 'USD')"#
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query!(
            r#"INSERT INTO expense
               (wallet_id, amount_amount, amount_currency, date, description,
                category_name, category_profit)
               VALUES (9002, 10.00, 'USD', DATE '2024-01-01', 'lunch', 'Food', false)"#
        )
        .execute(&pool)
        .await
        .unwrap();

        let renamed = repo(pool.clone())
            .rename("Food", false, "Groceries")
            .await
            .unwrap();

        assert_eq!(renamed.name, "Groceries");
        let category =
            sqlx::query_scalar!(r#"SELECT category_name FROM expense WHERE wallet_id = 9002"#)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(category, "Groceries");
    }

    /// Bypasses the service's advisory check, as a concurrent writer effectively does.
    #[sqlx::test(migrations = "../../migrations")]
    async fn the_database_rejects_a_case_variant_duplicate(pool: PgPool) {
        let repo = repo(pool);
        repo.insert(&category("Zebra", false)).await.unwrap();

        let err = repo.insert(&category("ZEBRA", false)).await.unwrap_err();

        assert!(matches!(err, PortError::Conflict(_)), "got {err:?}");
        assert!(repo.insert(&category("ZEBRA", true)).await.is_ok());
    }

    #[test]
    fn migrations_path_is_declared_once() {
        assert_eq!(MIGRATIONS, "../../migrations");
    }
}
