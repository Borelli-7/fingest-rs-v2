use fingest_kernel::{CategoryRef, DateRange, DomainError, Money};
use serde::{Deserialize, Serialize};

/// A spending allowance for one category over a period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    pub id: Option<i32>,
    pub category: CategoryRef,
    pub total: Money,
    pub date_range: DateRange,
}

impl Budget {
    pub fn new(
        category: CategoryRef,
        total: Money,
        date_range: DateRange,
    ) -> Result<Self, DomainError> {
        total.require_non_negative()?;
        date_range.require_valid()?;

        Ok(Self {
            id: None,
            category,
            total,
            date_range,
        })
    }

    pub fn with_id(mut self, id: i32) -> Self {
        self.id = Some(id);
        self
    }

    /// How much of the allowance is left. Goes negative when overspent, which is the
    /// signal the caller wants.
    pub fn left(&self, spent: &Money) -> Result<Money, DomainError> {
        self.total.sub(spent)
    }
}

/// A budget together with the spending recorded against it.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetWithSpent {
    pub budget: Budget,
    pub spent: Money,
}

impl BudgetWithSpent {
    pub fn left(&self) -> Result<Money, DomainError> {
        self.budget.left(&self.spent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use chrono::NaiveDate;
    use fingest_kernel::Currency;

    fn money(n: i64, code: &str) -> Money {
        Money::new(BigDecimal::from(n), Currency::new(code).unwrap())
    }

    fn pln(n: i64) -> Money {
        money(n, "PLN")
    }

    fn range() -> DateRange {
        DateRange::new(
            Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
        )
    }

    fn food() -> CategoryRef {
        CategoryRef::new("Food", false).unwrap()
    }

    #[test]
    fn left_is_the_unspent_remainder() {
        let budget = Budget::new(food(), pln(500), range()).unwrap();

        assert_eq!(budget.left(&pln(120)).unwrap(), pln(380));
    }

    #[test]
    fn overspending_yields_a_negative_remainder() {
        let budget = Budget::new(food(), pln(100), range()).unwrap();

        let left = budget.left(&pln(150)).unwrap();

        assert!(left.is_negative());
        assert_eq!(left, pln(-50));
    }

    #[test]
    fn a_negative_total_is_rejected() {
        assert_eq!(
            Budget::new(food(), pln(-1), range()).unwrap_err(),
            DomainError::NegativeAmount
        );
    }

    #[test]
    fn an_inverted_date_range_is_rejected() {
        let inverted = DateRange::new(
            Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
            Some(NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
        );

        assert!(matches!(
            Budget::new(food(), pln(100), inverted).unwrap_err(),
            DomainError::InvalidDateRange { .. }
        ));
    }

    #[test]
    fn spending_in_another_currency_is_rejected() {
        let budget = Budget::new(food(), pln(500), range()).unwrap();

        assert!(budget.left(&money(10, "USD")).is_err());
    }

    /// v1 serialises the output with `date_range`, but accepts input as `dateRange`.
    #[test]
    fn output_wire_shape_uses_snake_case() {
        let json = serde_json::to_value(Budget::new(food(), pln(500), range()).unwrap()).unwrap();

        assert!(json.get("date_range").is_some());
        assert!(json.get("dateRange").is_none());
        assert_eq!(json.as_object().unwrap().len(), 4);
    }
}
