use fingest_kernel::{Currency, DomainError, Money};
use serde::{Deserialize, Serialize};

use crate::expense::Expense;

/// A balance owned by one or more accounts.
///
/// The wallet is the consistency boundary for its own balance: every mutation goes through
/// a method here that returns the delta to persist, so no caller can invent one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wallet {
    pub id: Option<i32>,
    pub name: String,
    pub amount: Money,
}

impl Wallet {
    pub const MAX_NAME_LEN: usize = 255;

    pub fn new(name: impl Into<String>, amount: Money) -> Result<Self, DomainError> {
        amount.require_non_negative()?;

        let name = name.into();
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err(DomainError::EmptyField {
                field: "Wallet name",
            });
        }
        if trimmed.chars().count() > Self::MAX_NAME_LEN {
            return Err(DomainError::FieldTooLong {
                field: "Wallet name",
                max: Self::MAX_NAME_LEN,
            });
        }

        Ok(Self {
            id: None,
            name: trimmed.to_owned(),
            amount,
        })
    }

    pub fn with_id(mut self, id: i32) -> Self {
        self.id = Some(id);
        self
    }

    pub fn currency(&self) -> &Currency {
        &self.amount.currency
    }

    pub fn rename(&mut self, name: impl Into<String>) -> Result<(), DomainError> {
        let renamed = Self::new(name, self.amount.clone())?;
        self.name = renamed.name;
        Ok(())
    }

    /// Deviation D8: v1 accepted a mismatched currency and then updated the balance with
    /// `WHERE amount_currency = $1`, which matched no row — the entry was recorded and the
    /// balance silently drifted. Rejecting is the only way to keep the balance meaningful.
    pub fn require_compatible(&self, amount: &Money) -> Result<(), DomainError> {
        if amount.currency != self.amount.currency {
            return Err(DomainError::CurrencyMismatch {
                expected: self.amount.currency.to_string(),
                actual: amount.currency.to_string(),
            });
        }
        Ok(())
    }

    /// Applies an entry and returns the delta that must be persisted alongside it.
    pub fn apply(&mut self, expense: &Expense) -> Result<Money, DomainError> {
        self.require_compatible(&expense.amount)?;

        let delta = expense.balance_delta();
        self.amount = self.amount.add(&delta)?;
        Ok(delta)
    }

    /// Undoes a previously applied entry.
    pub fn reverse(&mut self, expense: &Expense) -> Result<Money, DomainError> {
        self.require_compatible(&expense.amount)?;

        let delta = expense.balance_delta().negate();
        self.amount = self.amount.add(&delta)?;
        Ok(delta)
    }

    /// Swaps one entry for another in a single step, returning the net delta.
    pub fn replace(&mut self, old: &Expense, new: &Expense) -> Result<Money, DomainError> {
        self.require_compatible(&old.amount)?;
        self.require_compatible(&new.amount)?;

        let delta = new.balance_delta().sub(&old.balance_delta())?;
        self.amount = self.amount.add(&delta)?;
        Ok(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use chrono::NaiveDate;
    use fingest_kernel::CategoryRef;

    fn money(n: i64, code: &str) -> Money {
        Money::new(BigDecimal::from(n), Currency::new(code).unwrap())
    }

    fn pln(n: i64) -> Money {
        money(n, "PLN")
    }

    fn wallet(balance: i64) -> Wallet {
        Wallet::new("Main", pln(balance)).unwrap()
    }

    fn entry(amount: Money, profit: bool) -> Expense {
        Expense::new(
            amount,
            NaiveDate::from_ymd_opt(2024, 6, 15).unwrap(),
            "entry",
            CategoryRef::new(if profit { "Salary" } else { "Food" }, profit).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn spending_reduces_the_balance() {
        let mut wallet = wallet(100);

        let delta = wallet.apply(&entry(pln(30), false)).unwrap();

        assert_eq!(delta, pln(-30));
        assert_eq!(wallet.amount, pln(70));
    }

    #[test]
    fn income_increases_the_balance() {
        let mut wallet = wallet(100);

        let delta = wallet.apply(&entry(pln(30), true)).unwrap();

        assert_eq!(delta, pln(30));
        assert_eq!(wallet.amount, pln(130));
    }

    #[test]
    fn a_balance_may_go_negative() {
        let mut wallet = wallet(10);

        wallet.apply(&entry(pln(25), false)).unwrap();

        assert_eq!(wallet.amount, pln(-15));
    }

    /// D8: the v1 silent-drift bug.
    #[test]
    fn a_mismatched_currency_is_rejected() {
        let mut wallet = wallet(100);

        let err = wallet.apply(&entry(money(30, "USD"), false)).unwrap_err();

        assert_eq!(
            err,
            DomainError::CurrencyMismatch {
                expected: "PLN".into(),
                actual: "USD".into()
            }
        );
        assert_eq!(wallet.amount, pln(100), "balance must be untouched");
    }

    #[test]
    fn reverse_undoes_apply() {
        let mut wallet = wallet(100);
        let expense = entry(pln(30), false);

        wallet.apply(&expense).unwrap();
        let delta = wallet.reverse(&expense).unwrap();

        assert_eq!(delta, pln(30));
        assert_eq!(wallet.amount, pln(100));
    }

    #[test]
    fn replace_applies_only_the_net_difference() {
        let mut wallet = wallet(100);
        let old = entry(pln(30), false);
        wallet.apply(&old).unwrap();

        let delta = wallet.replace(&old, &entry(pln(50), false)).unwrap();

        assert_eq!(delta, pln(-20));
        assert_eq!(wallet.amount, pln(50));
    }

    #[test]
    fn replace_handles_a_flip_from_spending_to_income() {
        let mut wallet = wallet(100);
        let old = entry(pln(30), false);
        wallet.apply(&old).unwrap();

        let delta = wallet.replace(&old, &entry(pln(30), true)).unwrap();

        assert_eq!(delta, pln(60), "reverse 30 out, then 30 in");
        assert_eq!(wallet.amount, pln(130));
    }

    #[test]
    fn creation_rejects_a_negative_opening_balance() {
        assert_eq!(
            Wallet::new("Main", pln(-1)).unwrap_err(),
            DomainError::NegativeAmount
        );
    }

    #[test]
    fn creation_rejects_a_blank_name() {
        assert!(Wallet::new("   ", pln(0)).is_err());
    }

    #[test]
    fn rename_keeps_the_balance() {
        let mut wallet = wallet(100);

        wallet.rename("  Savings  ").unwrap();

        assert_eq!(wallet.name, "Savings");
        assert_eq!(wallet.amount, pln(100));
    }

    #[test]
    fn rename_rejects_a_blank_name() {
        let mut wallet = wallet(100);
        assert!(wallet.rename("").is_err());
        assert_eq!(wallet.name, "Main");
    }

    #[test]
    fn wire_shape_matches_v1() {
        let json = serde_json::to_value(wallet(100).with_id(3)).unwrap();

        assert_eq!(json["id"], 3);
        assert_eq!(json["name"], "Main");
        assert_eq!(json["amount"]["currency"], "PLN");
        assert_eq!(json.as_object().unwrap().len(), 3);
    }
}
