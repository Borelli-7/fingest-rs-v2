use std::sync::Arc;

use fingest_kernel::PortError;

use crate::{category::Category, error::CatalogError, port::CategoryRepository};

/// The duplicate check above is advisory; the storage constraint is authoritative. A
/// concurrent writer that wins the race surfaces here, and gets the same v1 message.
fn lost_race(err: PortError, name: &str, profit: bool) -> CatalogError {
    match err {
        PortError::Conflict(_) => CatalogError::already_exists(name, profit),
        other => other.into(),
    }
}

/// Catalog use cases.
///
/// Check ordering mirrors v1 exactly: the handler validated the DTO before calling the
/// service, so a malformed name yields 400 even when the target does not exist (404).
pub struct CategoryService {
    repository: Arc<dyn CategoryRepository>,
}

impl CategoryService {
    pub fn new(repository: Arc<dyn CategoryRepository>) -> Self {
        Self { repository }
    }

    pub async fn list(&self) -> Result<Vec<Category>, CatalogError> {
        Ok(self.repository.list().await?)
    }

    pub async fn create(&self, name: &str, profit: bool) -> Result<Category, CatalogError> {
        let category = Category::new(name, profit)?;

        if self
            .repository
            .find_conflict(&category.name, profit, None)
            .await?
            .is_some()
        {
            return Err(CatalogError::already_exists(&category.name, profit));
        }

        self.repository
            .insert(&category)
            .await
            .map_err(|e| lost_race(e, &category.name, profit))
    }

    pub async fn rename(
        &self,
        name: &str,
        profit: bool,
        new_name: &str,
    ) -> Result<Category, CatalogError> {
        let renamed = Category::new(new_name, profit)?;

        if self.repository.find(name, profit).await?.is_none() {
            return Err(CatalogError::not_found(name, profit));
        }

        if self
            .repository
            .find_conflict(&renamed.name, profit, Some(name))
            .await?
            .is_some()
        {
            return Err(CatalogError::already_exists(&renamed.name, profit));
        }

        self.repository
            .rename(name, profit, &renamed.name)
            .await
            .map_err(|e| lost_race(e, &renamed.name, profit))
    }

    pub async fn delete(&self, name: &str, profit: bool) -> Result<(), CatalogError> {
        if self.repository.find(name, profit).await?.is_none() {
            return Err(CatalogError::not_found(name, profit));
        }

        if self.repository.delete(name, profit).await? == 0 {
            // v1 surfaced this as a 500 rather than a 404: the row vanished mid-request.
            return Err(CatalogError::Internal(
                "Failed to delete category".to_owned(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FailingCategoryRepository, InMemoryCategoryRepository};
    use fingest_kernel::PortError;
    use futures_executor::block_on;

    fn category(name: &str, profit: bool) -> Category {
        Category::new(name, profit).expect("valid fixture")
    }

    fn service_with(
        repo: Arc<InMemoryCategoryRepository>,
    ) -> (CategoryService, Arc<InMemoryCategoryRepository>) {
        (CategoryService::new(repo.clone()), repo)
    }

    #[test]
    fn list_returns_every_row() {
        let (svc, _) = service_with(Arc::new(InMemoryCategoryRepository::seeded([
            category("Food", false),
            category("Salary", true),
        ])));

        let found = block_on(svc.list()).unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn create_persists_the_category() {
        let (svc, repo) = service_with(Arc::new(InMemoryCategoryRepository::new()));

        let created = block_on(svc.create("Food", false)).unwrap();

        assert_eq!(created, category("Food", false));
        assert!(repo.contains("Food", false));
    }

    #[test]
    fn create_rejects_case_insensitive_duplicate() {
        let (svc, repo) = service_with(Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )])));

        let err = block_on(svc.create("FOOD", false)).unwrap_err();

        assert_eq!(err, CatalogError::already_exists("FOOD", false));
        assert_eq!(repo.len(), 1, "must not insert on conflict");
    }

    #[test]
    fn create_allows_same_name_with_different_profit_flag() {
        let (svc, repo) = service_with(Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )])));

        block_on(svc.create("Food", true)).unwrap();

        assert_eq!(repo.len(), 2);
    }

    #[test]
    fn create_rejects_invalid_name() {
        let (svc, repo) = service_with(Arc::new(InMemoryCategoryRepository::new()));

        let err = block_on(svc.create("   ", false)).unwrap_err();

        assert!(matches!(err, CatalogError::Validation(_)));
        assert!(repo.is_empty());
    }

    #[test]
    fn rename_updates_the_row() {
        let (svc, repo) = service_with(Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )])));

        let renamed = block_on(svc.rename("Food", false, "Groceries")).unwrap();

        assert_eq!(renamed.name, "Groceries");
        assert!(repo.contains("Groceries", false));
        assert!(!repo.contains("Food", false));
    }

    #[test]
    fn rename_reports_missing_target() {
        let (svc, _) = service_with(Arc::new(InMemoryCategoryRepository::new()));

        let err = block_on(svc.rename("Food", false, "Groceries")).unwrap_err();

        assert_eq!(err, CatalogError::not_found("Food", false));
    }

    #[test]
    fn rename_rejects_collision_with_another_category() {
        let (svc, _) = service_with(Arc::new(InMemoryCategoryRepository::seeded([
            category("Food", false),
            category("Transport", false),
        ])));

        let err = block_on(svc.rename("Food", false, "TRANSPORT")).unwrap_err();

        assert_eq!(err, CatalogError::already_exists("TRANSPORT", false));
    }

    #[test]
    fn rename_to_its_own_name_is_allowed() {
        let (svc, _) = service_with(Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )])));

        assert!(block_on(svc.rename("Food", false, "Food")).is_ok());
    }

    /// v1 validated the DTO in the handler, so a bad name beats a missing target.
    #[test]
    fn rename_validation_precedes_existence_check() {
        let (svc, _) = service_with(Arc::new(InMemoryCategoryRepository::new()));

        let err = block_on(svc.rename("Missing", false, "")).unwrap_err();

        assert!(
            matches!(err, CatalogError::Validation(_)),
            "expected 400 before 404, got {err:?}"
        );
    }

    #[test]
    fn delete_removes_the_row() {
        let (svc, repo) = service_with(Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )])));

        block_on(svc.delete("Food", false)).unwrap();

        assert!(repo.is_empty());
    }

    #[test]
    fn delete_reports_missing_target() {
        let (svc, _) = service_with(Arc::new(InMemoryCategoryRepository::new()));

        let err = block_on(svc.delete("Food", false)).unwrap_err();

        assert_eq!(err, CatalogError::not_found("Food", false));
    }

    #[test]
    fn port_failures_propagate_untouched() {
        let svc = CategoryService::new(Arc::new(FailingCategoryRepository(PortError::Storage(
            "connection reset".into(),
        ))));

        let err = block_on(svc.list()).unwrap_err();

        assert_eq!(
            err,
            CatalogError::Port(PortError::Storage("connection reset".into()))
        );
    }
}
