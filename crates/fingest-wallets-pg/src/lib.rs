//! Postgres adapter for the wallets context.

mod mapping;
mod reader;
mod unit_of_work;

pub use reader::PgWalletReader;
pub use unit_of_work::{PgUnitOfWork, PgWalletTx};

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use chrono::{NaiveDate, Utc};
    use fingest_kernel::{
        CategoryRef, Currency, DateRange, DomainEvent, EventEnvelope, Money, PortError,
    };
    use fingest_wallets_core::{Expense, UnitOfWork, Wallet, WalletReader};
    use sqlx::PgPool;

    const OWNER: &str = "user1";

    fn pln(n: i64) -> Money {
        Money::new(BigDecimal::from(n), Currency::default())
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()
    }

    fn food() -> CategoryRef {
        CategoryRef::new("Food", false).unwrap()
    }

    fn entry(amount: i64) -> Expense {
        Expense::new(pln(amount), day(), "lunch", food()).unwrap()
    }

    /// Creates a wallet owned by `user1` with the given balance, through the real adapter.
    async fn seed_wallet(pool: &PgPool, balance: i64) -> i32 {
        let uow = PgUnitOfWork::new(pool.clone());
        let mut tx = uow.begin().await.unwrap();
        let id = tx
            .insert_wallet(OWNER, &Wallet::new("Main", pln(balance)).unwrap())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn balance_of(pool: &PgPool, wallet_id: i32) -> BigDecimal {
        sqlx::query_scalar!(
            r#"SELECT amount_amount AS "amount!" FROM wallet WHERE id = $1"#,
            wallet_id
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn expense_count(pool: &PgPool, wallet_id: i32) -> i64 {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!" FROM expense WHERE wallet_id = $1"#,
            wallet_id
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn outbox_count(pool: &PgPool) -> i64 {
        sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!" FROM outbox"#)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn a_new_wallet_is_linked_to_its_owner(pool: PgPool) {
        let id = seed_wallet(&pool, 100).await;

        let found = PgWalletReader::new(pool.clone())
            .find_owned(OWNER, id)
            .await
            .unwrap();

        assert_eq!(found.unwrap().amount, pln(100));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn another_user_cannot_see_the_wallet(pool: PgPool) {
        let id = seed_wallet(&pool, 100).await;

        let found = PgWalletReader::new(pool)
            .find_owned("user2", id)
            .await
            .unwrap();

        assert!(found.is_none());
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn expense_and_balance_move_together_on_commit(pool: PgPool) {
        let wallet_id = seed_wallet(&pool, 100).await;
        let uow = PgUnitOfWork::new(pool.clone());

        let mut tx = uow.begin().await.unwrap();
        tx.insert_expense(wallet_id, &entry(30)).await.unwrap();
        tx.adjust_balance(wallet_id, &pln(-30)).await.unwrap();
        tx.commit().await.unwrap();

        assert_eq!(balance_of(&pool, wallet_id).await, BigDecimal::from(70));
        assert_eq!(expense_count(&pool, wallet_id).await, 1);
    }

    /// The guarantee v1 lacked: without a commit, neither the row nor the balance survives.
    #[sqlx::test(migrations = "../../migrations")]
    async fn dropping_the_transaction_rolls_everything_back(pool: PgPool) {
        let wallet_id = seed_wallet(&pool, 100).await;
        let uow = PgUnitOfWork::new(pool.clone());

        {
            let mut tx = uow.begin().await.unwrap();
            tx.insert_expense(wallet_id, &entry(30)).await.unwrap();
            tx.adjust_balance(wallet_id, &pln(-30)).await.unwrap();
            // dropped without commit
        }

        assert_eq!(
            balance_of(&pool, wallet_id).await,
            BigDecimal::from(100),
            "balance must be untouched"
        );
        assert_eq!(expense_count(&pool, wallet_id).await, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn events_land_in_the_outbox_with_the_same_transaction(pool: PgPool) {
        let wallet_id = seed_wallet(&pool, 100).await;
        let uow = PgUnitOfWork::new(pool.clone());
        let before = outbox_count(&pool).await;

        let event = DomainEvent::ExpenseRecorded {
            wallet_id,
            expense_id: 1,
            amount: pln(30),
            category_name: "Food".into(),
            category_profit: false,
        };
        let envelope = EventEnvelope::new(&event, Utc::now()).unwrap();

        let mut tx = uow.begin().await.unwrap();
        tx.append_events(std::slice::from_ref(&envelope))
            .await
            .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(outbox_count(&pool).await, before + 1);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn outbox_rows_roll_back_with_their_transaction(pool: PgPool) {
        let uow = PgUnitOfWork::new(pool.clone());
        let before = outbox_count(&pool).await;

        let event = DomainEvent::WalletCreated {
            login: OWNER.into(),
            wallet_id: 1,
        };
        let envelope = EventEnvelope::new(&event, Utc::now()).unwrap();

        {
            let mut tx = uow.begin().await.unwrap();
            tx.append_events(std::slice::from_ref(&envelope))
                .await
                .unwrap();
        }

        assert_eq!(outbox_count(&pool).await, before);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn category_existence_is_checked_against_the_real_table(pool: PgPool) {
        let uow = PgUnitOfWork::new(pool);
        let mut tx = uow.begin().await.unwrap();

        assert!(tx.category_exists(&food()).await.unwrap());
        assert!(
            !tx.category_exists(&CategoryRef::new("Nonsense", false).unwrap())
                .await
                .unwrap()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn expenses_are_filtered_by_date_range(pool: PgPool) {
        let wallet_id = seed_wallet(&pool, 100).await;
        let uow = PgUnitOfWork::new(pool.clone());

        let mut tx = uow.begin().await.unwrap();
        tx.insert_expense(wallet_id, &entry(30)).await.unwrap();
        tx.commit().await.unwrap();

        let reader = PgWalletReader::new(pool);

        let inside = DateRange::new(
            Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
        );
        assert_eq!(
            reader
                .list_expenses(wallet_id, &inside)
                .await
                .unwrap()
                .len(),
            1
        );

        let outside = DateRange::new(
            Some(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()),
            Some(NaiveDate::from_ymd_opt(2020, 12, 31).unwrap()),
        );
        assert!(
            reader
                .list_expenses(wallet_id, &outside)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn highest_expense_ignores_income(pool: PgPool) {
        let wallet_id = seed_wallet(&pool, 100).await;
        let uow = PgUnitOfWork::new(pool.clone());

        let income = Expense::new(
            pln(500),
            day(),
            "payday",
            CategoryRef::new("Salary", true).unwrap(),
        )
        .unwrap();

        let mut tx = uow.begin().await.unwrap();
        tx.insert_expense(wallet_id, &entry(30)).await.unwrap();
        tx.insert_expense(wallet_id, &income).await.unwrap();
        tx.commit().await.unwrap();

        let highest = PgWalletReader::new(pool)
            .highest_expense(wallet_id, &DateRange::new(None, None))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(highest.amount, pln(30));
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn deleting_a_wallet_cascades_to_its_expenses(pool: PgPool) {
        let wallet_id = seed_wallet(&pool, 100).await;
        let uow = PgUnitOfWork::new(pool.clone());

        let mut tx = uow.begin().await.unwrap();
        tx.insert_expense(wallet_id, &entry(30)).await.unwrap();
        tx.commit().await.unwrap();

        let mut tx = uow.begin().await.unwrap();
        assert_eq!(tx.delete_wallet(wallet_id).await.unwrap(), 1);
        tx.commit().await.unwrap();

        assert_eq!(expense_count(&pool, wallet_id).await, 0);
    }

    #[sqlx::test(migrations = "../../migrations")]
    async fn an_unknown_category_is_rejected_by_the_foreign_key(pool: PgPool) {
        let wallet_id = seed_wallet(&pool, 100).await;
        let uow = PgUnitOfWork::new(pool.clone());

        let orphan = Expense::new(
            pln(10),
            day(),
            "x",
            CategoryRef::new("Nonsense", false).unwrap(),
        )
        .unwrap();

        let mut tx = uow.begin().await.unwrap();
        let err = tx.insert_expense(wallet_id, &orphan).await.unwrap_err();

        assert!(matches!(err, PortError::Conflict(_)), "got {err:?}");
    }
}
