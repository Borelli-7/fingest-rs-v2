use std::sync::Arc;

use fingest_kernel::{CategoryRef, Clock, DateRange, Money};

use crate::{
    budget::{Budget, BudgetWithSpent},
    error::PlanningError,
    port::BudgetRepository,
};

/// Partial budget update; absent fields keep their current value.
#[derive(Default)]
pub struct BudgetPatch {
    pub category: Option<CategoryRef>,
    pub total: Option<Money>,
    pub date_range: Option<DateRange>,
}

pub struct BudgetService {
    budgets: Arc<dyn BudgetRepository>,
    clock: Arc<dyn Clock>,
}

impl BudgetService {
    pub fn new(budgets: Arc<dyn BudgetRepository>, clock: Arc<dyn Clock>) -> Self {
        Self { budgets, clock }
    }

    async fn require_user(&self, login: &str) -> Result<(), PlanningError> {
        if !self.budgets.owner_exists(login).await? {
            return Err(PlanningError::user_not_found(login));
        }
        Ok(())
    }

    async fn require_category(&self, category: &CategoryRef) -> Result<(), PlanningError> {
        if !self.budgets.category_exists(category).await? {
            return Err(PlanningError::unknown_category(
                &category.name,
                category.profit,
            ));
        }
        Ok(())
    }

    /// Loads a budget and proves `login` owns it.
    async fn require_owned(
        &self,
        login: &str,
        budget_id: i32,
        action: &str,
    ) -> Result<Budget, PlanningError> {
        let (budget, owner) = self
            .budgets
            .find_with_owner(budget_id)
            .await?
            .ok_or_else(|| PlanningError::budget_not_found(budget_id))?;

        if owner != login {
            return Err(PlanningError::not_owner(action));
        }

        Ok(budget)
    }

    pub async fn list(
        &self,
        login: &str,
        start_window: &DateRange,
        end_window: &DateRange,
    ) -> Result<Vec<BudgetWithSpent>, PlanningError> {
        self.require_user(login).await?;

        Ok(self
            .budgets
            .list_with_spent(login, start_window, end_window)
            .await?)
    }

    /// Deviation D7: v1 returned a fabricated `id: 1` and never touched the database.
    pub async fn create(
        &self,
        login: &str,
        category: CategoryRef,
        total: Money,
        date_range: DateRange,
    ) -> Result<Budget, PlanningError> {
        self.require_user(login).await?;

        let budget = Budget::new(category, total, date_range)?;
        self.require_category(&budget.category).await?;

        let id = self
            .budgets
            .insert(login, &budget, self.clock.now_utc())
            .await?;

        Ok(budget.with_id(id))
    }

    pub async fn update(
        &self,
        login: &str,
        budget_id: i32,
        patch: BudgetPatch,
    ) -> Result<Budget, PlanningError> {
        self.require_user(login).await?;
        let existing = self.require_owned(login, budget_id, "update").await?;

        let updated = Budget::new(
            patch.category.unwrap_or_else(|| existing.category.clone()),
            patch.total.unwrap_or_else(|| existing.total.clone()),
            patch
                .date_range
                .unwrap_or_else(|| existing.date_range.clone()),
        )?
        .with_id(budget_id);

        self.require_category(&updated.category).await?;

        if self.budgets.update(&updated).await? == 0 {
            return Err(PlanningError::budget_not_found(budget_id));
        }

        Ok(updated)
    }

    pub async fn delete(&self, login: &str, budget_id: i32) -> Result<(), PlanningError> {
        self.require_user(login).await?;
        self.require_owned(login, budget_id, "delete").await?;

        if self.budgets.delete(budget_id).await? == 0 {
            return Err(PlanningError::budget_not_found(budget_id));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::InMemoryBudgetRepository;
    use bigdecimal::BigDecimal;
    use chrono::{NaiveDate, TimeZone, Utc};
    use fingest_kernel::{Currency, FixedClock};
    use futures_executor::block_on;

    const OWNER: &str = "bob";

    fn pln(n: i64) -> Money {
        Money::new(BigDecimal::from(n), Currency::default())
    }

    fn range() -> DateRange {
        DateRange::new(
            Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
        )
    }

    fn any_window() -> DateRange {
        DateRange::new(None, None)
    }

    fn food() -> CategoryRef {
        CategoryRef::new("Food", false).unwrap()
    }

    fn repo() -> Arc<InMemoryBudgetRepository> {
        Arc::new(
            InMemoryBudgetRepository::new()
                .with_owner(OWNER)
                .with_owner("mallory")
                .with_category("Food", false),
        )
    }

    fn service(repo: Arc<InMemoryBudgetRepository>) -> BudgetService {
        BudgetService::new(
            repo,
            Arc::new(FixedClock(Utc.timestamp_opt(1_700_000_000, 0).unwrap())),
        )
    }

    #[test]
    fn create_persists_a_real_row_and_returns_its_id() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));

        let created = block_on(svc.create(OWNER, food(), pln(500), range())).unwrap();

        assert!(created.id.is_some());
        assert_eq!(repo.len(), 1, "v1 never actually wrote the row");
        assert_eq!(repo.event_types(), vec!["BudgetCreated"]);
    }

    #[test]
    fn create_rejects_an_unknown_category() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));

        let err = block_on(svc.create(
            OWNER,
            CategoryRef::new("Nonsense", false).unwrap(),
            pln(500),
            range(),
        ))
        .unwrap_err();

        assert_eq!(err, PlanningError::unknown_category("Nonsense", false));
        assert_eq!(repo.len(), 0);
    }

    #[test]
    fn create_rejects_an_inverted_range() {
        let inverted = DateRange::new(
            Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
            Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
        );
        let repo = repo();

        let err = block_on(service(Arc::clone(&repo)).create(OWNER, food(), pln(1), inverted))
            .unwrap_err();

        assert!(matches!(err, PlanningError::Validation(_)));
        assert_eq!(repo.len(), 0);
    }

    #[test]
    fn create_requires_the_user_to_exist() {
        let err = block_on(service(repo()).create("ghost", food(), pln(1), range())).unwrap_err();

        assert_eq!(err, PlanningError::user_not_found("ghost"));
    }

    #[test]
    fn list_reports_spending_and_remainder() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));
        let created = block_on(svc.create(OWNER, food(), pln(500), range())).unwrap();
        repo.set_spent(created.id.unwrap(), pln(120));

        let listed = block_on(svc.list(OWNER, &any_window(), &any_window())).unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].spent, pln(120));
        assert_eq!(listed[0].left().unwrap(), pln(380));
    }

    #[test]
    fn list_only_returns_the_callers_budgets() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));
        block_on(svc.create(OWNER, food(), pln(500), range())).unwrap();
        block_on(svc.create("mallory", food(), pln(999), range())).unwrap();

        let listed = block_on(svc.list(OWNER, &any_window(), &any_window())).unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].budget.total, pln(500));
    }

    #[test]
    fn update_changes_the_total() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));
        let created = block_on(svc.create(OWNER, food(), pln(500), range())).unwrap();

        let updated = block_on(svc.update(
            OWNER,
            created.id.unwrap(),
            BudgetPatch {
                total: Some(pln(750)),
                ..Default::default()
            },
        ))
        .unwrap();

        assert_eq!(updated.total, pln(750));
        assert_eq!(updated.category, food(), "untouched fields are preserved");
    }

    #[test]
    fn a_stranger_cannot_update_a_budget() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));
        let created = block_on(svc.create(OWNER, food(), pln(500), range())).unwrap();

        let err = block_on(svc.update(
            "mallory",
            created.id.unwrap(),
            BudgetPatch {
                total: Some(pln(1)),
                ..Default::default()
            },
        ))
        .unwrap_err();

        assert_eq!(err, PlanningError::not_owner("update"));
    }

    #[test]
    fn updating_an_unknown_budget_is_404() {
        let err = block_on(service(repo()).update(OWNER, 999, BudgetPatch::default())).unwrap_err();

        assert_eq!(err, PlanningError::budget_not_found(999));
    }

    #[test]
    fn delete_removes_the_budget() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));
        let created = block_on(svc.create(OWNER, food(), pln(500), range())).unwrap();

        block_on(svc.delete(OWNER, created.id.unwrap())).unwrap();

        assert_eq!(repo.len(), 0);
    }

    #[test]
    fn a_stranger_cannot_delete_a_budget() {
        let repo = repo();
        let svc = service(Arc::clone(&repo));
        let created = block_on(svc.create(OWNER, food(), pln(500), range())).unwrap();

        let err = block_on(svc.delete("mallory", created.id.unwrap())).unwrap_err();

        assert_eq!(err, PlanningError::not_owner("delete"));
        assert_eq!(repo.len(), 1);
    }
}
