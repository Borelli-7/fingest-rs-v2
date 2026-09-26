use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use fingest_kernel::{CategoryRef, Clock, DateRange, DomainEvent, EventEnvelope, Money, PortError};

use crate::{
    error::WalletsError,
    expense::Expense,
    port::{UnitOfWork, WalletReader},
    summary::{Summary, count_by_category},
    wallet::Wallet,
};

/// Unvalidated expense input from the API boundary.
pub struct NewExpense {
    pub amount: Money,
    pub date: NaiveDate,
    pub description: String,
    pub category: CategoryRef,
}

/// Partial expense update; absent fields keep their current value.
#[derive(Default)]
pub struct ExpensePatch {
    pub amount: Option<Money>,
    pub date: Option<NaiveDate>,
    pub description: Option<String>,
    pub category: Option<CategoryRef>,
}

/// Partial wallet update.
#[derive(Default)]
pub struct WalletPatch {
    pub name: Option<String>,
    pub amount: Option<Money>,
}

pub struct WalletService {
    reader: Arc<dyn WalletReader>,
    unit_of_work: Arc<dyn UnitOfWork>,
    clock: Arc<dyn Clock>,
}

fn envelopes(events: &[DomainEvent], at: DateTime<Utc>) -> Result<Vec<EventEnvelope>, PortError> {
    events
        .iter()
        .map(|event| EventEnvelope::new(event, at))
        .collect()
}

impl WalletService {
    pub fn new(
        reader: Arc<dyn WalletReader>,
        unit_of_work: Arc<dyn UnitOfWork>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            reader,
            unit_of_work,
            clock,
        }
    }

    async fn require_user(&self, login: &str) -> Result<(), WalletsError> {
        if !self.reader.owner_exists(login).await? {
            return Err(WalletsError::user_not_found(login));
        }
        Ok(())
    }

    /// Reports a wallet the caller does not own as missing, so the response never confirms
    /// that an id exists. Used by the expense routes, matching v1.
    async fn require_owned(&self, login: &str, wallet_id: i32) -> Result<Wallet, WalletsError> {
        self.reader
            .find_owned(login, wallet_id)
            .await?
            .ok_or_else(|| WalletsError::wallet_not_found_for_user(wallet_id, login))
    }

    /// v1's wallet and expense-update routes distinguish the two cases: an existing wallet
    /// owned by someone else is 403, a wallet that does not exist at all is 404.
    async fn require_owned_distinguishing(
        &self,
        login: &str,
        wallet_id: i32,
        forbidden_message: &str,
    ) -> Result<Wallet, WalletsError> {
        if let Some(wallet) = self.reader.find_owned(login, wallet_id).await? {
            return Ok(wallet);
        }

        if self.reader.wallet_exists(wallet_id).await? {
            Err(WalletsError::Forbidden(forbidden_message.to_owned()))
        } else {
            Err(WalletsError::wallet_not_found(wallet_id))
        }
    }

    // --- wallets ---

    pub async fn list_wallets(&self, login: &str) -> Result<Vec<Wallet>, WalletsError> {
        self.require_user(login).await?;
        Ok(self.reader.list_for_owner(login).await?)
    }

    pub async fn create_wallet(
        &self,
        login: &str,
        name: String,
        amount: Money,
    ) -> Result<Wallet, WalletsError> {
        self.require_user(login).await?;
        let wallet = Wallet::new(name, amount)?;

        let mut tx = self.unit_of_work.begin().await?;
        let id = tx.insert_wallet(login, &wallet).await?;

        let events = [DomainEvent::WalletCreated {
            login: login.to_owned(),
            wallet_id: id,
        }];
        tx.append_events(&envelopes(&events, self.clock.now_utc())?)
            .await?;
        tx.commit().await?;

        Ok(wallet.with_id(id))
    }

    /// The wallet is re-read under lock inside the transaction; v1-style "read, mutate,
    /// write every column" let a concurrent expense's balance change be overwritten.
    pub async fn update_wallet(
        &self,
        login: &str,
        wallet_id: i32,
        patch: WalletPatch,
    ) -> Result<Wallet, WalletsError> {
        self.require_user(login).await?;
        self.require_owned_distinguishing(login, wallet_id, "Not authorized to update this wallet")
            .await?;

        let mut tx = self.unit_of_work.begin().await?;
        let Some(mut wallet) = tx.find_owned(login, wallet_id).await? else {
            return Err(WalletsError::wallet_not_found(wallet_id));
        };

        if let Some(name) = patch.name {
            wallet.rename(name)?;
            tx.rename_wallet(wallet_id, &wallet.name).await?;
        }

        if let Some(amount) = patch.amount {
            let delta = wallet.set_balance(amount)?;
            if delta != Money::zero(delta.currency.clone()) {
                tx.adjust_balance(wallet_id, &delta).await?;

                let events = [DomainEvent::WalletBalanceAdjusted {
                    wallet_id,
                    delta,
                    balance: wallet.amount.clone(),
                }];
                tx.append_events(&envelopes(&events, self.clock.now_utc())?)
                    .await?;
            }
        }

        let updated = [DomainEvent::WalletUpdated {
            login: login.to_owned(),
            wallet_id,
        }];
        tx.append_events(&envelopes(&updated, self.clock.now_utc())?)
            .await?;
        tx.commit().await?;

        Ok(wallet)
    }

    pub async fn delete_wallet(&self, login: &str, wallet_id: i32) -> Result<(), WalletsError> {
        self.require_user(login).await?;
        self.require_owned_distinguishing(login, wallet_id, "Not authorized to delete this wallet")
            .await?;

        let mut tx = self.unit_of_work.begin().await?;
        if tx.delete_wallet(wallet_id).await? == 0 {
            return Err(WalletsError::wallet_not_found(wallet_id));
        }
        let events = [DomainEvent::WalletDeleted {
            login: login.to_owned(),
            wallet_id,
        }];
        tx.append_events(&envelopes(&events, self.clock.now_utc())?)
            .await?;
        tx.commit().await?;

        Ok(())
    }

    // --- reads over a date range ---

    pub async fn list_expenses(
        &self,
        login: &str,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Vec<Expense>, WalletsError> {
        self.require_user(login).await?;
        self.require_owned(login, wallet_id).await?;
        Ok(self.reader.list_expenses(wallet_id, range).await?)
    }

    pub async fn highest_expense(
        &self,
        login: &str,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Option<Expense>, WalletsError> {
        self.require_user(login).await?;
        self.require_owned(login, wallet_id).await?;
        Ok(self.reader.highest_expense(wallet_id, range).await?)
    }

    /// Deviation D6: v1 ignored the range and returned only the balance.
    pub async fn summary(
        &self,
        login: &str,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<Summary, WalletsError> {
        self.require_user(login).await?;
        let wallet = self.require_owned(login, wallet_id).await?;
        let expenses = self.reader.list_expenses(wallet_id, range).await?;

        Ok(Summary::build(
            wallet.name.clone(),
            wallet.amount.clone(),
            &expenses,
        )?)
    }

    /// Deviation D5: v1 always returned an empty map.
    pub async fn counted_categories(
        &self,
        login: &str,
        wallet_id: i32,
        range: &DateRange,
    ) -> Result<std::collections::HashMap<String, i64>, WalletsError> {
        self.require_user(login).await?;
        self.require_owned(login, wallet_id).await?;
        let expenses = self.reader.list_expenses(wallet_id, range).await?;

        Ok(count_by_category(&expenses))
    }

    // --- expenses (transactional) ---

    /// Inserts the entry, moves the balance and records the events in one transaction.
    ///
    /// v1 ran the insert and the balance update as two independent statements, so a failure
    /// between them left the balance permanently wrong.
    pub async fn record_expense(
        &self,
        login: &str,
        wallet_id: i32,
        input: NewExpense,
    ) -> Result<Expense, WalletsError> {
        let expense = Expense::new(input.amount, input.date, input.description, input.category)?;

        let mut tx = self.unit_of_work.begin().await?;

        let Some(mut wallet) = tx.find_owned(login, wallet_id).await? else {
            return Err(WalletsError::wallet_not_found_for_user(wallet_id, login));
        };
        if !tx.category_exists(&expense.category).await? {
            return Err(WalletsError::unknown_category(
                &expense.category.name,
                expense.category.profit,
            ));
        }

        let delta = wallet.apply(&expense)?;

        let expense_id = tx.insert_expense(wallet_id, &expense).await?;
        tx.adjust_balance(wallet_id, &delta).await?;

        let events = [
            DomainEvent::ExpenseRecorded {
                wallet_id,
                expense_id,
                amount: expense.amount.clone(),
                category_name: expense.category.name.clone(),
                category_profit: expense.category.profit,
            },
            DomainEvent::WalletBalanceAdjusted {
                wallet_id,
                delta,
                balance: wallet.amount.clone(),
            },
        ];
        tx.append_events(&envelopes(&events, self.clock.now_utc())?)
            .await?;
        tx.commit().await?;

        Ok(expense.with_id(expense_id))
    }

    pub async fn update_expense(
        &self,
        login: &str,
        wallet_id: i32,
        expense_id: i32,
        patch: ExpensePatch,
    ) -> Result<Expense, WalletsError> {
        self.require_user(login).await?;
        self.require_owned_distinguishing(
            login,
            wallet_id,
            "Not authorized to update expenses in this wallet",
        )
        .await?;

        let mut tx = self.unit_of_work.begin().await?;

        let Some(mut wallet) = tx.find_owned(login, wallet_id).await? else {
            return Err(WalletsError::wallet_not_found_for_user(wallet_id, login));
        };
        let existing = tx
            .find_expense(wallet_id, expense_id)
            .await?
            .ok_or_else(|| WalletsError::expense_not_found(expense_id))?;

        let updated = Expense::new(
            patch.amount.unwrap_or_else(|| existing.amount.clone()),
            patch.date.unwrap_or(existing.date),
            patch
                .description
                .unwrap_or_else(|| existing.description.clone()),
            patch.category.unwrap_or_else(|| existing.category.clone()),
        )?
        .with_id(expense_id);

        if !tx.category_exists(&updated.category).await? {
            return Err(WalletsError::unknown_category(
                &updated.category.name,
                updated.category.profit,
            ));
        }

        let delta = wallet.replace(&existing, &updated)?;

        if tx.update_expense(&updated).await? == 0 {
            return Err(WalletsError::expense_not_found(expense_id));
        }
        tx.adjust_balance(wallet_id, &delta).await?;

        let events = [
            DomainEvent::ExpenseUpdated {
                wallet_id,
                expense_id,
            },
            DomainEvent::WalletBalanceAdjusted {
                wallet_id,
                delta,
                balance: wallet.amount.clone(),
            },
        ];
        tx.append_events(&envelopes(&events, self.clock.now_utc())?)
            .await?;
        tx.commit().await?;

        Ok(updated)
    }

    pub async fn delete_expense(
        &self,
        login: &str,
        wallet_id: i32,
        expense_id: i32,
    ) -> Result<(), WalletsError> {
        let mut tx = self.unit_of_work.begin().await?;

        let Some(mut wallet) = tx.find_owned(login, wallet_id).await? else {
            return Err(WalletsError::wallet_not_found_for_user(wallet_id, login));
        };
        let existing = tx
            .find_expense(wallet_id, expense_id)
            .await?
            .ok_or_else(|| WalletsError::expense_not_found(expense_id))?;

        // Removing an entry must undo its effect on the balance; v1 left the balance stale.
        let delta = wallet.reverse(&existing)?;

        if tx.delete_expense(wallet_id, expense_id).await? == 0 {
            return Err(WalletsError::expense_not_found(expense_id));
        }
        tx.adjust_balance(wallet_id, &delta).await?;

        let events = [
            DomainEvent::ExpenseRemoved {
                wallet_id,
                expense_id,
            },
            DomainEvent::WalletBalanceAdjusted {
                wallet_id,
                delta,
                balance: wallet.amount.clone(),
            },
        ];
        tx.append_events(&envelopes(&events, self.clock.now_utc())?)
            .await?;
        tx.commit().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{InMemoryStore, InMemoryUnitOfWork};
    use bigdecimal::BigDecimal;
    use chrono::TimeZone;
    use fingest_kernel::{Currency, FixedClock};
    use futures_executor::block_on;

    const OWNER: &str = "bob";
    const WALLET: i32 = 1;

    fn money(n: i64, code: &str) -> Money {
        Money::new(BigDecimal::from(n), Currency::new(code).unwrap())
    }

    fn pln(n: i64) -> Money {
        money(n, "PLN")
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()
    }

    fn food() -> CategoryRef {
        CategoryRef::new("Food", false).unwrap()
    }

    fn salary() -> CategoryRef {
        CategoryRef::new("Salary", true).unwrap()
    }

    fn new_expense(amount: i64, category: CategoryRef) -> NewExpense {
        NewExpense {
            amount: pln(amount),
            date: day(),
            description: "entry".to_owned(),
            category,
        }
    }

    /// Store seeded with one owner, one 100 PLN wallet and the two categories.
    fn store() -> Arc<InMemoryStore> {
        Arc::new(
            InMemoryStore::new()
                .with_owner(OWNER)
                .with_category("Food", false)
                .with_category("Salary", true)
                .with_wallet(
                    OWNER,
                    Wallet::new("Main", pln(100)).unwrap().with_id(WALLET),
                ),
        )
    }

    fn service(store: Arc<InMemoryStore>) -> WalletService {
        let clock = Arc::new(FixedClock(Utc.timestamp_opt(1_700_000_000, 0).unwrap()));
        WalletService::new(
            Arc::clone(&store) as Arc<dyn WalletReader>,
            Arc::new(InMemoryUnitOfWork::new(store)),
            clock,
        )
    }

    // --- wallets ---

    #[test]
    fn listing_requires_the_user_to_exist() {
        let err = block_on(service(store()).list_wallets("ghost")).unwrap_err();
        assert_eq!(err, WalletsError::user_not_found("ghost"));
    }

    #[test]
    fn create_wallet_persists_and_emits_an_event() {
        let store = store();
        let created =
            block_on(service(Arc::clone(&store)).create_wallet(OWNER, "Savings".into(), pln(50)))
                .unwrap();

        assert!(created.id.is_some());
        assert_eq!(store.wallet_count(), 2);
        assert!(
            store
                .recorded_events()
                .contains(&"WalletCreated".to_owned())
        );
    }

    #[test]
    fn create_wallet_rejects_a_negative_opening_balance() {
        let store = store();
        let err =
            block_on(service(Arc::clone(&store)).create_wallet(OWNER, "Savings".into(), pln(-1)))
                .unwrap_err();

        assert!(matches!(err, WalletsError::Validation(_)));
        assert_eq!(store.wallet_count(), 1, "nothing may be written");
    }

    // --- update_wallet ---

    #[test]
    fn renaming_leaves_the_balance_alone_and_records_no_balance_event() {
        let store = store();

        let updated = block_on(service(Arc::clone(&store)).update_wallet(
            OWNER,
            WALLET,
            WalletPatch {
                name: Some("Daily".into()),
                amount: None,
            },
        ))
        .unwrap();

        assert_eq!(updated.name, "Daily");
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(100));
        assert_eq!(store.recorded_events(), vec!["WalletUpdated"]);
    }

    /// The rename must not write back the balance it read: a concurrent expense may have
    /// moved it in between.
    #[test]
    fn renaming_preserves_a_balance_changed_since_it_was_read() {
        let store = store();
        let svc = service(Arc::clone(&store));
        block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();

        block_on(svc.update_wallet(
            OWNER,
            WALLET,
            WalletPatch {
                name: Some("Daily".into()),
                amount: None,
            },
        ))
        .unwrap();

        assert_eq!(store.balance_of(WALLET).unwrap(), pln(70));
    }

    #[test]
    fn setting_the_amount_moves_the_balance_by_a_delta_and_records_it() {
        let store = store();

        block_on(service(Arc::clone(&store)).update_wallet(
            OWNER,
            WALLET,
            WalletPatch {
                name: None,
                amount: Some(pln(250)),
            },
        ))
        .unwrap();

        assert_eq!(store.balance_of(WALLET).unwrap(), pln(250));
        assert_eq!(
            store.recorded_events(),
            vec!["WalletBalanceAdjusted", "WalletUpdated"]
        );
    }

    #[test]
    fn switching_the_wallet_currency_is_rejected_and_nothing_is_written() {
        let store = store();

        let err = block_on(service(Arc::clone(&store)).update_wallet(
            OWNER,
            WALLET,
            WalletPatch {
                name: Some("Dollars".into()),
                amount: Some(money(100, "USD")),
            },
        ))
        .unwrap_err();

        assert!(matches!(err, WalletsError::BadRequest(_)));
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(100));
        assert!(store.recorded_events().is_empty());
    }

    // --- record_expense ---

    #[test]
    fn updating_a_wallet_emits_wallet_updated() {
        let store = store();

        block_on(service(Arc::clone(&store)).update_wallet(
            OWNER,
            WALLET,
            WalletPatch {
                name: Some("Daily".into()),
                amount: None,
            },
        ))
        .unwrap();

        assert_eq!(store.recorded_events(), vec!["WalletUpdated"]);
    }

    #[test]
    fn deleting_a_wallet_emits_wallet_deleted() {
        let store = store();

        block_on(service(Arc::clone(&store)).delete_wallet(OWNER, WALLET)).unwrap();

        assert_eq!(store.wallet_count(), 0);
        assert_eq!(store.recorded_events(), vec!["WalletDeleted"]);
    }

    #[test]
    fn recording_spending_moves_the_balance_in_one_transaction() {
        let store = store();

        let recorded = block_on(service(Arc::clone(&store)).record_expense(
            OWNER,
            WALLET,
            new_expense(30, food()),
        ))
        .unwrap();

        assert!(recorded.id.is_some());
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(70));
        assert_eq!(store.expense_count(), 1);
        assert_eq!(
            store.recorded_events(),
            vec!["ExpenseRecorded", "WalletBalanceAdjusted"]
        );
    }

    #[test]
    fn recording_income_increases_the_balance() {
        let store = store();

        block_on(service(Arc::clone(&store)).record_expense(
            OWNER,
            WALLET,
            new_expense(30, salary()),
        ))
        .unwrap();

        assert_eq!(store.balance_of(WALLET).unwrap(), pln(130));
    }

    /// D8, and the whole point of the UnitOfWork: a rejected entry must leave *nothing*
    /// behind — no row, no balance movement, no event.
    #[test]
    fn a_currency_mismatch_rolls_the_whole_transaction_back() {
        let store = store();
        let input = NewExpense {
            amount: money(30, "USD"),
            date: day(),
            description: "entry".to_owned(),
            category: food(),
        };

        let err =
            block_on(service(Arc::clone(&store)).record_expense(OWNER, WALLET, input)).unwrap_err();

        assert!(matches!(err, WalletsError::BadRequest(_)));
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(100));
        assert_eq!(store.expense_count(), 0);
        assert!(store.recorded_events().is_empty());
    }

    #[test]
    fn an_unknown_category_is_rejected_and_nothing_is_written() {
        let store = store();
        let input = new_expense(30, CategoryRef::new("Nonsense", false).unwrap());

        let err =
            block_on(service(Arc::clone(&store)).record_expense(OWNER, WALLET, input)).unwrap_err();

        assert_eq!(err, WalletsError::unknown_category("Nonsense", false));
        assert_eq!(store.expense_count(), 0);
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(100));
    }

    #[test]
    fn recording_against_a_wallet_you_do_not_own_is_404() {
        let store = Arc::new(
            InMemoryStore::new()
                .with_owner(OWNER)
                .with_owner("mallory")
                .with_category("Food", false)
                .with_wallet(
                    OWNER,
                    Wallet::new("Main", pln(100)).unwrap().with_id(WALLET),
                ),
        );

        let err = block_on(service(Arc::clone(&store)).record_expense(
            "mallory",
            WALLET,
            new_expense(30, food()),
        ))
        .unwrap_err();

        assert_eq!(
            err,
            WalletsError::wallet_not_found_for_user(WALLET, "mallory")
        );
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(100));
    }

    #[test]
    fn a_negative_amount_is_rejected_before_any_lookup() {
        let store = store();
        let input = NewExpense {
            amount: pln(-5),
            date: day(),
            description: "entry".to_owned(),
            category: food(),
        };

        assert!(matches!(
            block_on(service(Arc::clone(&store)).record_expense(OWNER, WALLET, input)).unwrap_err(),
            WalletsError::Validation(_)
        ));
        assert_eq!(store.expense_count(), 0);
    }

    // --- delete / update ---

    #[test]
    fn deleting_an_entry_restores_the_balance() {
        let store = store();
        let svc = service(Arc::clone(&store));
        let recorded =
            block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(70));

        block_on(svc.delete_expense(OWNER, WALLET, recorded.id.unwrap())).unwrap();

        assert_eq!(
            store.balance_of(WALLET).unwrap(),
            pln(100),
            "v1 left the balance stale after a delete"
        );
        assert_eq!(store.expense_count(), 0);
    }

    #[test]
    fn deleting_an_unknown_entry_is_404() {
        let store = store();
        let err = block_on(service(store).delete_expense(OWNER, WALLET, 999)).unwrap_err();

        assert_eq!(err, WalletsError::expense_not_found(999));
    }

    #[test]
    fn updating_an_entry_applies_only_the_net_difference() {
        let store = store();
        let svc = service(Arc::clone(&store));
        let recorded =
            block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();

        block_on(svc.update_expense(
            OWNER,
            WALLET,
            recorded.id.unwrap(),
            ExpensePatch {
                amount: Some(pln(50)),
                ..Default::default()
            },
        ))
        .unwrap();

        assert_eq!(store.balance_of(WALLET).unwrap(), pln(50));
    }

    #[test]
    fn updating_only_the_description_leaves_the_balance_alone() {
        let store = store();
        let svc = service(Arc::clone(&store));
        let recorded =
            block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();

        let updated = block_on(svc.update_expense(
            OWNER,
            WALLET,
            recorded.id.unwrap(),
            ExpensePatch {
                description: Some("renamed".into()),
                ..Default::default()
            },
        ))
        .unwrap();

        assert_eq!(updated.description, "renamed");
        assert_eq!(updated.amount, pln(30));
        assert_eq!(store.balance_of(WALLET).unwrap(), pln(70));
    }

    #[test]
    fn flipping_an_entry_to_income_nets_correctly() {
        let store = store();
        let svc = service(Arc::clone(&store));
        let recorded =
            block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();

        block_on(svc.update_expense(
            OWNER,
            WALLET,
            recorded.id.unwrap(),
            ExpensePatch {
                category: Some(salary()),
                ..Default::default()
            },
        ))
        .unwrap();

        assert_eq!(store.balance_of(WALLET).unwrap(), pln(130));
    }

    // --- reads (D5, D6) ---

    #[test]
    fn summary_groups_by_category_over_the_range() {
        let store = store();
        let svc = service(Arc::clone(&store));
        block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();
        block_on(svc.record_expense(OWNER, WALLET, new_expense(20, food()))).unwrap();
        block_on(svc.record_expense(OWNER, WALLET, new_expense(500, salary()))).unwrap();

        let summary = block_on(svc.summary(OWNER, WALLET, &DateRange::new(None, None))).unwrap();

        assert_eq!(summary.wallet_name, "Main");
        assert_eq!(summary.expense_categories["Food"], pln(50));
        assert_eq!(summary.income_categories["Salary"], pln(500));
        assert_eq!(summary.total_expense, pln(50));
        assert_eq!(summary.total_income, pln(500));
    }

    #[test]
    fn summary_honours_the_date_range() {
        let store = store();
        let svc = service(Arc::clone(&store));
        block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();

        let outside = DateRange::new(
            Some(NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()),
            Some(NaiveDate::from_ymd_opt(2020, 12, 31).unwrap()),
        );
        let summary = block_on(svc.summary(OWNER, WALLET, &outside)).unwrap();

        assert!(summary.expense_categories.is_empty());
        assert_eq!(summary.total_expense, pln(0));
        assert_eq!(
            summary.balance,
            pln(70),
            "balance still reflects all entries"
        );
    }

    #[test]
    fn counted_categories_tallies_spending_entries() {
        let store = store();
        let svc = service(Arc::clone(&store));
        block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();
        block_on(svc.record_expense(OWNER, WALLET, new_expense(20, food()))).unwrap();
        block_on(svc.record_expense(OWNER, WALLET, new_expense(500, salary()))).unwrap();

        let counts =
            block_on(svc.counted_categories(OWNER, WALLET, &DateRange::new(None, None))).unwrap();

        assert_eq!(counts["Food"], 2);
        assert!(!counts.contains_key("Salary"), "income is excluded");
    }

    #[test]
    fn highest_expense_ignores_income() {
        let store = store();
        let svc = service(Arc::clone(&store));
        block_on(svc.record_expense(OWNER, WALLET, new_expense(30, food()))).unwrap();
        block_on(svc.record_expense(OWNER, WALLET, new_expense(500, salary()))).unwrap();

        let highest =
            block_on(svc.highest_expense(OWNER, WALLET, &DateRange::new(None, None))).unwrap();

        assert_eq!(highest.unwrap().amount, pln(30));
    }
}
