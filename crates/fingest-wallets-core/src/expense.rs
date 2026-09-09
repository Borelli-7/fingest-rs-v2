use chrono::NaiveDate;
use fingest_kernel::{CategoryRef, DomainError, Money};
use serde::{Deserialize, Serialize};

/// A single movement of money in or out of a wallet.
///
/// `amount` is always non-negative; direction comes from `category.profit`. Storing a
/// signed amount would allow a "negative income", which has no meaning here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expense {
    pub id: Option<i32>,
    pub amount: Money,
    pub date: NaiveDate,
    pub description: String,
    pub category: CategoryRef,
}

impl Expense {
    pub const MAX_DESCRIPTION_LEN: usize = 255;

    pub fn new(
        amount: Money,
        date: NaiveDate,
        description: impl Into<String>,
        category: CategoryRef,
    ) -> Result<Self, DomainError> {
        amount.require_non_negative()?;

        let description = description.into();
        let trimmed = description.trim();

        if trimmed.is_empty() {
            return Err(DomainError::EmptyField {
                field: "Description",
            });
        }
        if trimmed.chars().count() > Self::MAX_DESCRIPTION_LEN {
            return Err(DomainError::FieldTooLong {
                field: "Description",
                max: Self::MAX_DESCRIPTION_LEN,
            });
        }

        Ok(Self {
            id: None,
            amount,
            date,
            description: trimmed.to_owned(),
            category,
        })
    }

    pub fn with_id(mut self, id: i32) -> Self {
        self.id = Some(id);
        self
    }

    pub fn is_income(&self) -> bool {
        self.category.is_income()
    }

    /// How this entry moves a balance: positive for income, negative for spending.
    pub fn balance_delta(&self) -> Money {
        if self.is_income() {
            self.amount.clone()
        } else {
            self.amount.negate()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use fingest_kernel::Currency;

    fn pln(n: i64) -> Money {
        Money::new(BigDecimal::from(n), Currency::default())
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()
    }

    fn food() -> CategoryRef {
        CategoryRef::new("Food", false).unwrap()
    }

    fn salary() -> CategoryRef {
        CategoryRef::new("Salary", true).unwrap()
    }

    #[test]
    fn spending_reduces_a_balance() {
        let expense = Expense::new(pln(50), date(), "lunch", food()).unwrap();

        assert!(!expense.is_income());
        assert_eq!(expense.balance_delta(), pln(-50));
    }

    #[test]
    fn income_increases_a_balance() {
        let income = Expense::new(pln(50), date(), "payday", salary()).unwrap();

        assert!(income.is_income());
        assert_eq!(income.balance_delta(), pln(50));
    }

    #[test]
    fn a_negative_amount_is_rejected() {
        assert_eq!(
            Expense::new(pln(-1), date(), "refund", food()).unwrap_err(),
            DomainError::NegativeAmount
        );
    }

    #[test]
    fn a_blank_description_is_rejected() {
        assert!(Expense::new(pln(1), date(), "   ", food()).is_err());
    }

    #[test]
    fn an_overlong_description_is_rejected() {
        let long = "x".repeat(Expense::MAX_DESCRIPTION_LEN + 1);
        assert_eq!(
            Expense::new(pln(1), date(), long, food()).unwrap_err(),
            DomainError::FieldTooLong {
                field: "Description",
                max: 255
            }
        );
    }

    #[test]
    fn description_is_trimmed() {
        let expense = Expense::new(pln(1), date(), "  lunch  ", food()).unwrap();
        assert_eq!(expense.description, "lunch");
    }

    #[test]
    fn wire_shape_matches_v1() {
        let expense = Expense::new(pln(50), date(), "lunch", food())
            .unwrap()
            .with_id(7);
        let json = serde_json::to_value(&expense).unwrap();

        assert_eq!(json["id"], 7);
        assert_eq!(json["date"], "2024-06-15");
        assert_eq!(json["description"], "lunch");
        assert_eq!(json["category"]["name"], "Food");
        assert_eq!(json["amount"]["currency"], "PLN");
    }
}
