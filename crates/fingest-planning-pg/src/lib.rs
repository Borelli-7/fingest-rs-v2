//! Postgres adapter for the planning context.

use async_trait::async_trait;
use bigdecimal::BigDecimal;
use chrono::NaiveDate;
use fingest_kernel::{CategoryRef, Currency, DateRange, EventEnvelope, Money, PortError};
use fingest_planning_core::{
    Budget, BudgetRepository, BudgetWithSpent, EventsForId, Saving, SavingRepository,
};
use sqlx::PgPool;

fn money(amount: BigDecimal, currency: &str) -> Result<Money, PortError> {
    let currency = Currency::new(currency)
        .map_err(|e| PortError::Storage(format!("stored currency is invalid: {e}")))?;
    Ok(Money::new(amount, currency))
}

/// Built field-by-field: the row is persisted, so a tightened domain rule must not make it
/// unreadable.
#[allow(clippy::too_many_arguments)]
fn budget(
    id: i32,
    category_name: String,
    category_profit: bool,
    total_amount: BigDecimal,
    total_currency: String,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<Budget, PortError> {
    Ok(Budget {
        id: Some(id),
        category: CategoryRef {
            name: category_name,
            profit: category_profit,
        },
        total: money(total_amount, &total_currency)?,
        date_range: DateRange::new(Some(start_date), Some(end_date)),
    })
}

fn to_port_error(err: sqlx::Error) -> PortError {
    if let sqlx::Error::Database(ref db_err) = err {
        match db_err.code().as_deref() {
            Some("23505") => return PortError::Conflict("Record already exists".to_owned()),
            Some("23503") => {
                return PortError::Conflict(
                    "Referenced record does not exist or is still in use".to_owned(),
                );
            }
            _ => {}
        }
    }
    PortError::Storage(err.to_string())
}

pub struct PgBudgetRepository {
    pool: PgPool,
}

impl PgBudgetRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BudgetRepository for PgBudgetRepository {
    async fn owner_exists(&self, login: &str) -> Result<bool, PortError> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM account WHERE login = $1) AS "exists!""#,
            login
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn category_exists(&self, category: &CategoryRef) -> Result<bool, PortError> {
        sqlx::query_scalar!(
            r#"SELECT EXISTS(
                   SELECT 1 FROM category WHERE name = $1 AND profit = $2
               ) AS "exists!""#,
            category.name,
            category.profit
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    /// Spending is summed across every wallet the owner holds, restricted to the budget's
    /// own period and to non-income entries.
    async fn list_with_spent(
        &self,
        login: &str,
        start_window: &DateRange,
        end_window: &DateRange,
    ) -> Result<Vec<BudgetWithSpent>, PortError> {
        let rows = sqlx::query!(
            r#"
            SELECT b.id, b.category_name, b.category_profit,
                   b.total_amount, b.total_currency, b.start_date, b.end_date,
                   COALESCE(SUM(
                       CASE WHEN e.category_profit = false
                             AND e.date BETWEEN b.start_date AND b.end_date
                            THEN e.amount_amount ELSE 0 END
                   ), 0) AS "spent_amount!"
            FROM budget b
            LEFT JOIN account_wallet aw ON aw.account_login = b.account_login
            LEFT JOIN expense e ON e.wallet_id = aw.wallet_id
                                AND e.category_name = b.category_name
            WHERE b.account_login = $1
              AND b.start_date BETWEEN $2 AND $3
              AND b.end_date BETWEEN $4 AND $5
            GROUP BY b.id, b.category_name, b.category_profit,
                     b.total_amount, b.total_currency, b.start_date, b.end_date
            ORDER BY b.start_date DESC, b.id
            "#,
            login,
            start_window.start,
            start_window.end,
            end_window.start,
            end_window.end
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_port_error)?;

        rows.into_iter()
            .map(|row| {
                let spent = money(row.spent_amount, &row.total_currency)?;
                Ok(BudgetWithSpent {
                    budget: budget(
                        row.id,
                        row.category_name,
                        row.category_profit,
                        row.total_amount,
                        row.total_currency,
                        row.start_date,
                        row.end_date,
                    )?,
                    spent,
                })
            })
            .collect()
    }

    async fn find_with_owner(&self, budget_id: i32) -> Result<Option<(Budget, String)>, PortError> {
        let row = sqlx::query!(
            r#"SELECT id, account_login, category_name, category_profit,
                      total_amount, total_currency, start_date, end_date
               FROM budget WHERE id = $1"#,
            budget_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(to_port_error)?;

        row.map(|row| {
            let owner = row.account_login;
            Ok((
                budget(
                    row.id,
                    row.category_name,
                    row.category_profit,
                    row.total_amount,
                    row.total_currency,
                    row.start_date,
                    row.end_date,
                )?,
                owner,
            ))
        })
        .transpose()
    }

    async fn insert(
        &self,
        login: &str,
        new: &Budget,
        events: EventsForId<'_>,
    ) -> Result<i32, PortError> {
        let mut tx = self.pool.begin().await.map_err(to_port_error)?;

        let id = sqlx::query_scalar!(
            r#"INSERT INTO budget
               (account_login, category_name, category_profit,
                total_amount, total_currency, start_date, end_date)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING id"#,
            login,
            new.category.name,
            new.category.profit,
            new.total.amount,
            new.total.currency.as_str(),
            new.date_range.start,
            new.date_range.end
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(to_port_error)?;

        append_events(&mut tx, &events(id)?).await?;
        tx.commit().await.map_err(to_port_error)?;

        Ok(id)
    }

    async fn update(&self, updated: &Budget, events: &[EventEnvelope]) -> Result<u64, PortError> {
        let mut tx = self.pool.begin().await.map_err(to_port_error)?;

        let rows = sqlx::query!(
            r#"UPDATE budget
               SET category_name = $1, category_profit = $2,
                   total_amount = $3, total_currency = $4,
                   start_date = $5, end_date = $6
               WHERE id = $7"#,
            updated.category.name,
            updated.category.profit,
            updated.total.amount,
            updated.total.currency.as_str(),
            updated.date_range.start,
            updated.date_range.end,
            updated.id
        )
        .execute(&mut *tx)
        .await
        .map(|r| r.rows_affected())
        .map_err(to_port_error)?;

        if rows > 0 {
            append_events(&mut tx, events).await?;
            tx.commit().await.map_err(to_port_error)?;
        }
        Ok(rows)
    }

    async fn delete(&self, budget_id: i32, events: &[EventEnvelope]) -> Result<u64, PortError> {
        let mut tx = self.pool.begin().await.map_err(to_port_error)?;

        let rows = sqlx::query!(r#"DELETE FROM budget WHERE id = $1"#, budget_id)
            .execute(&mut *tx)
            .await
            .map(|r| r.rows_affected())
            .map_err(to_port_error)?;

        if rows > 0 {
            append_events(&mut tx, events).await?;
            tx.commit().await.map_err(to_port_error)?;
        }
        Ok(rows)
    }
}

async fn append_events(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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

pub struct PgSavingRepository {
    pool: PgPool,
}

impl PgSavingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SavingRepository for PgSavingRepository {
    async fn list_for_owner(&self, login: &str) -> Result<Vec<Saving>, PortError> {
        let rows = sqlx::query!(
            r#"SELECT id, name, goal_amount, goal_currency,
                      current_amount, current_currency, start_date, end_date
               FROM saving WHERE account_login = $1 ORDER BY id"#,
            login
        )
        .fetch_all(&self.pool)
        .await
        .map_err(to_port_error)?;

        rows.into_iter()
            .map(|row| {
                Ok(Saving {
                    id: Some(row.id),
                    name: row.name,
                    goal: money(row.goal_amount, &row.goal_currency)?,
                    current: money(row.current_amount, &row.current_currency)?,
                    date_range: DateRange::new(Some(row.start_date), Some(row.end_date)),
                })
            })
            .collect()
    }

    async fn insert(&self, login: &str, new: &Saving) -> Result<i32, PortError> {
        sqlx::query_scalar!(
            r#"INSERT INTO saving
               (account_login, name, goal_amount, goal_currency,
                current_amount, current_currency, start_date, end_date)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               RETURNING id"#,
            login,
            new.name,
            new.goal.amount,
            new.goal.currency.as_str(),
            new.current.amount,
            new.current.currency.as_str(),
            new.date_range.start,
            new.date_range.end
        )
        .fetch_one(&self.pool)
        .await
        .map_err(to_port_error)
    }

    async fn delete(&self, saving_id: i32) -> Result<u64, PortError> {
        sqlx::query!(r#"DELETE FROM saving WHERE id = $1"#, saving_id)
            .execute(&self.pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(to_port_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "user1";

    fn pln(n: i64) -> Money {
        Money::new(BigDecimal::from(n), Currency::default())
    }

    fn range(from: (i32, u32, u32), to: (i32, u32, u32)) -> DateRange {
        DateRange::new(
            Some(NaiveDate::from_ymd_opt(from.0, from.1, from.2).unwrap()),
            Some(NaiveDate::from_ymd_opt(to.0, to.1, to.2).unwrap()),
        )
    }

    fn year_2024() -> DateRange {
        range((2024, 1, 1), (2024, 12, 31))
    }

    fn any_window() -> DateRange {
        DateRange::new(None, None)
    }

    fn food() -> CategoryRef {
        CategoryRef::new("Food", false).unwrap()
    }

    fn a_budget(total: i64) -> Budget {
        Budget::new(food(), pln(total), year_2024()).unwrap()
    }

    fn no_events(_: i32) -> Result<Vec<EventEnvelope>, PortError> {
        Ok(Vec::new())
    }

    fn created(budget_id: i32) -> Result<Vec<EventEnvelope>, PortError> {
        EventEnvelope::for_all(
            &[fingest_kernel::DomainEvent::BudgetCreated {
                login: OWNER.into(),
                budget_id,
            }],
            chrono::Utc::now(),
        )
    }

    fn one_event(event: fingest_kernel::DomainEvent) -> Vec<EventEnvelope> {
        EventEnvelope::for_all(&[event], chrono::Utc::now()).unwrap()
    }

    async fn outbox_types(pool: &PgPool) -> Vec<String> {
        sqlx::query_scalar!(r#"SELECT event_type FROM outbox ORDER BY id"#)
            .fetch_all(pool)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn insert_then_find_round_trips(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool);

        let id = repo
            .insert(OWNER, &a_budget(500), &no_events)
            .await
            .unwrap();

        let (found, owner) = repo.find_with_owner(id).await.unwrap().unwrap();
        assert_eq!(owner, OWNER);
        assert_eq!(found.total, pln(500));
        assert_eq!(found.category, food());
    }

    /// D7: v1 returned a fabricated id and wrote nothing.
    #[sqlx::test(migrations = "../../migrations")]
    async fn insert_writes_a_real_row_and_an_outbox_event(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool.clone());

        let id = repo.insert(OWNER, &a_budget(500), &created).await.unwrap();

        let rows = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM budget WHERE id = $1"#,
            id
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 1);

        let events = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM outbox WHERE event_type = 'BudgetCreated'"#
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(events, 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_and_delete_write_their_events_only_when_a_row_changed(pool: PgPool) {
        use fingest_kernel::DomainEvent::{BudgetDeleted, BudgetUpdated};

        let repo = PgBudgetRepository::new(pool.clone());
        let id = repo
            .insert(OWNER, &a_budget(500), &no_events)
            .await
            .unwrap();
        let login = OWNER.to_owned();

        let updated = one_event(BudgetUpdated {
            login: login.clone(),
            budget_id: id,
        });
        let deleted = one_event(BudgetDeleted {
            login,
            budget_id: id,
        });

        assert_eq!(
            repo.update(&a_budget(600).with_id(id), &updated)
                .await
                .unwrap(),
            1
        );
        assert_eq!(repo.delete(id, &deleted).await.unwrap(), 1);
        assert_eq!(
            repo.update(&a_budget(700).with_id(id), &updated)
                .await
                .unwrap(),
            0
        );
        assert_eq!(repo.delete(id, &deleted).await.unwrap(), 0);

        assert_eq!(
            outbox_types(&pool).await,
            ["BudgetUpdated", "BudgetDeleted"]
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn spending_is_summed_against_the_budget(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool.clone());
        let id = repo
            .insert(OWNER, &a_budget(500), &no_events)
            .await
            .unwrap();

        sqlx::query!(
            r#"INSERT INTO wallet (id, name, amount_amount, amount_currency)
               VALUES (9100, 'W', 1000.00, 'PLN')"#
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query!(
            r#"INSERT INTO account_wallet (account_login, wallet_id) VALUES ($1, 9100)"#,
            OWNER
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query!(
            r#"INSERT INTO expense
               (wallet_id, amount_amount, amount_currency, date, description,
                category_name, category_profit)
               VALUES (9100, 120.00, 'PLN', DATE '2024-06-15', 'lunch', 'Food', false)"#
        )
        .execute(&pool)
        .await
        .unwrap();

        let listed = repo
            .list_with_spent(OWNER, &any_window(), &any_window())
            .await
            .unwrap();

        let found = listed.iter().find(|b| b.budget.id == Some(id)).unwrap();
        assert_eq!(found.spent, pln(120));
        assert_eq!(found.left().unwrap(), pln(380));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_budget_with_no_spending_reports_zero(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool);
        let id = repo
            .insert(OWNER, &a_budget(500), &no_events)
            .await
            .unwrap();

        let listed = repo
            .list_with_spent(OWNER, &any_window(), &any_window())
            .await
            .unwrap();

        let found = listed.iter().find(|b| b.budget.id == Some(id)).unwrap();
        assert_eq!(found.spent, pln(0));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn the_start_window_filters_results(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool);
        repo.insert(OWNER, &a_budget(500), &no_events)
            .await
            .unwrap();

        let listed = repo
            .list_with_spent(OWNER, &range((2020, 1, 1), (2020, 12, 31)), &any_window())
            .await
            .unwrap();

        assert!(listed.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn another_users_budgets_are_not_listed(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool);
        repo.insert(OWNER, &a_budget(500), &no_events)
            .await
            .unwrap();

        let listed = repo
            .list_with_spent("user2", &any_window(), &any_window())
            .await
            .unwrap();

        assert!(listed.is_empty());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn update_and_delete_report_rows_affected(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool);
        let id = repo
            .insert(OWNER, &a_budget(500), &no_events)
            .await
            .unwrap();

        let changed = a_budget(750).with_id(id);
        assert_eq!(repo.update(&changed, &[]).await.unwrap(), 1);
        assert_eq!(
            repo.find_with_owner(id).await.unwrap().unwrap().0.total,
            pln(750)
        );

        assert_eq!(repo.delete(id, &[]).await.unwrap(), 1);
        assert_eq!(repo.delete(id, &[]).await.unwrap(), 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn an_unknown_category_is_rejected_by_the_foreign_key(pool: PgPool) {
        let repo = PgBudgetRepository::new(pool);
        let orphan = Budget::new(
            CategoryRef::new("Nonsense", false).unwrap(),
            pln(1),
            year_2024(),
        )
        .unwrap();

        let err = repo.insert(OWNER, &orphan, &no_events).await.unwrap_err();

        assert!(matches!(err, PortError::Conflict(_)), "got {err:?}");
    }

    // --- savings: persisted but unexposed ---

    #[sqlx::test(migrations = "../../migrations")]
    async fn savings_round_trip(pool: PgPool) {
        let repo = PgSavingRepository::new(pool);
        let saving = Saving::new("Holiday", pln(1000), pln(250), year_2024()).unwrap();

        let id = repo.insert(OWNER, &saving).await.unwrap();
        let listed = repo.list_for_owner(OWNER).await.unwrap();

        let found = listed.iter().find(|s| s.id == Some(id)).unwrap();
        assert_eq!(found.name, "Holiday");
        assert_eq!(found.remaining().unwrap(), pln(750));
        assert!(!found.is_reached());
    }
}
