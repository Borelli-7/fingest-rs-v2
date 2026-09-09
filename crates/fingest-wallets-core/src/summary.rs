use std::collections::HashMap;

use fingest_kernel::{Currency, DomainError, Money};
use serde::{Deserialize, Serialize};

use crate::expense::Expense;

/// Aggregated view of a wallet over a date range.
///
/// Deviation D6: v1 returned only the balance and ignored the range entirely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub wallet_name: String,
    pub balance: Money,
    pub expense_categories: HashMap<String, Money>,
    pub income_categories: HashMap<String, Money>,
    pub total_expense: Money,
    pub total_income: Money,
}

impl Summary {
    /// `balance` is the wallet's stored balance, not a recomputation: entries outside the
    /// requested range still affect it, so deriving it from `expenses` would be wrong.
    pub fn build(
        wallet_name: impl Into<String>,
        balance: Money,
        expenses: &[Expense],
    ) -> Result<Self, DomainError> {
        let currency = balance.currency.clone();

        let mut expense_categories: HashMap<String, Money> = HashMap::new();
        let mut income_categories: HashMap<String, Money> = HashMap::new();
        let mut total_expense = Money::zero(currency.clone());
        let mut total_income = Money::zero(currency.clone());

        for entry in expenses {
            if entry.amount.currency != currency {
                return Err(DomainError::CurrencyMismatch {
                    expected: currency.to_string(),
                    actual: entry.amount.currency.to_string(),
                });
            }

            let (bucket, total) = if entry.is_income() {
                (&mut income_categories, &mut total_income)
            } else {
                (&mut expense_categories, &mut total_expense)
            };

            let running = bucket
                .entry(entry.category.name.clone())
                .or_insert_with(|| Money::zero(currency.clone()));
            *running = running.add(&entry.amount)?;
            *total = total.add(&entry.amount)?;
        }

        Ok(Self {
            wallet_name: wallet_name.into(),
            balance,
            expense_categories,
            income_categories,
            total_expense,
            total_income,
        })
    }

    pub fn empty(wallet_name: impl Into<String>, balance: Money) -> Self {
        let currency: Currency = balance.currency.clone();
        Self {
            wallet_name: wallet_name.into(),
            balance,
            expense_categories: HashMap::new(),
            income_categories: HashMap::new(),
            total_expense: Money::zero(currency.clone()),
            total_income: Money::zero(currency),
        }
    }
}

/// Number of spending entries per category.
///
/// Deviation D5: v1 always returned `{}`. Confirmed as a *count*, not a sum. Income is
/// excluded because the map is keyed by category name alone, and a category's identity is
/// `(name, profit)` — including income would merge two distinct categories into one bucket.
pub fn count_by_category(expenses: &[Expense]) -> HashMap<String, i64> {
    let mut counts = HashMap::new();

    for entry in expenses.iter().filter(|e| !e.is_income()) {
        *counts.entry(entry.category.name.clone()).or_insert(0) += 1;
    }

    counts
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

    fn entry(amount: i64, category: &str, profit: bool) -> Expense {
        Expense::new(
            pln(amount),
            NaiveDate::from_ymd_opt(2024, 6, 15).unwrap(),
            "entry",
            CategoryRef::new(category, profit).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn splits_spending_from_income() {
        let entries = [
            entry(30, "Food", false),
            entry(20, "Food", false),
            entry(15, "Transport", false),
            entry(500, "Salary", true),
        ];

        let summary = Summary::build("Main", pln(1000), &entries).unwrap();

        assert_eq!(summary.expense_categories["Food"], pln(50));
        assert_eq!(summary.expense_categories["Transport"], pln(15));
        assert_eq!(summary.income_categories["Salary"], pln(500));
        assert_eq!(summary.total_expense, pln(65));
        assert_eq!(summary.total_income, pln(500));
    }

    #[test]
    fn balance_is_the_stored_value_not_a_recomputation() {
        let summary = Summary::build("Main", pln(1000), &[entry(30, "Food", false)]).unwrap();

        assert_eq!(
            summary.balance,
            pln(1000),
            "entries outside the range still affect the stored balance"
        );
    }

    #[test]
    fn a_category_appears_in_only_one_bucket() {
        let entries = [entry(10, "Food", false), entry(10, "Food", true)];

        let summary = Summary::build("Main", pln(0), &entries).unwrap();

        assert_eq!(summary.expense_categories["Food"], pln(10));
        assert_eq!(summary.income_categories["Food"], pln(10));
    }

    #[test]
    fn an_empty_range_yields_zero_totals_but_keeps_the_balance() {
        let summary = Summary::build("Main", pln(1000), &[]).unwrap();

        assert!(summary.expense_categories.is_empty());
        assert!(summary.income_categories.is_empty());
        assert_eq!(summary.total_expense, pln(0));
        assert_eq!(summary.total_income, pln(0));
        assert_eq!(summary.balance, pln(1000));
    }

    #[test]
    fn totals_carry_the_wallet_currency() {
        let summary = Summary::build("Main", money(100, "USD"), &[]).unwrap();

        assert_eq!(summary.total_expense.currency.as_str(), "USD");
        assert_eq!(summary.total_income.currency.as_str(), "USD");
    }

    #[test]
    fn a_foreign_currency_entry_is_rejected() {
        let entries = [Expense::new(
            money(10, "USD"),
            NaiveDate::from_ymd_opt(2024, 6, 15).unwrap(),
            "entry",
            CategoryRef::new("Food", false).unwrap(),
        )
        .unwrap()];

        assert!(Summary::build("Main", pln(1000), &entries).is_err());
    }

    #[test]
    fn wire_shape_matches_v1() {
        let json = serde_json::to_value(Summary::empty("Main", pln(10))).unwrap();

        for key in [
            "wallet_name",
            "balance",
            "expense_categories",
            "income_categories",
            "total_expense",
            "total_income",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert_eq!(json.as_object().unwrap().len(), 6);
    }

    // --- D5 ---

    #[test]
    fn counts_spending_entries_per_category() {
        let entries = [
            entry(30, "Food", false),
            entry(20, "Food", false),
            entry(15, "Transport", false),
        ];

        let counts = count_by_category(&entries);

        assert_eq!(counts["Food"], 2);
        assert_eq!(counts["Transport"], 1);
    }

    #[test]
    fn counting_ignores_income() {
        let counts = count_by_category(&[entry(500, "Salary", true), entry(30, "Food", false)]);

        assert_eq!(counts.len(), 1);
        assert_eq!(counts["Food"], 1);
    }

    #[test]
    fn counting_is_a_tally_not_a_sum() {
        let counts = count_by_category(&[entry(1000, "Food", false)]);

        assert_eq!(counts["Food"], 1, "one entry, regardless of its amount");
    }

    #[test]
    fn categories_with_no_entries_are_absent() {
        assert!(count_by_category(&[]).is_empty());
    }
}
