use fingest_kernel::{DateRange, DomainError, Money};
use serde::{Deserialize, Serialize};

/// A savings goal.
///
/// Carried forward from v1's schema and model but **deliberately not exposed over HTTP** —
/// v1 had no savings endpoints either. Keeping the aggregate and its port means adding
/// routes later is additive; `routes_do_not_expose_savings` guards against it leaking in
/// unnoticed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saving {
    pub id: Option<i32>,
    pub name: String,
    pub goal: Money,
    pub current: Money,
    pub date_range: DateRange,
}

impl Saving {
    pub const MAX_NAME_LEN: usize = 255;

    pub fn new(
        name: impl Into<String>,
        goal: Money,
        current: Money,
        date_range: DateRange,
    ) -> Result<Self, DomainError> {
        goal.require_non_negative()?;
        current.require_non_negative()?;
        date_range.require_valid()?;

        // Comparing the two also proves they share a currency.
        goal.sub(&current)?;

        let name = name.into();
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err(DomainError::EmptyField {
                field: "Saving name",
            });
        }
        if trimmed.chars().count() > Self::MAX_NAME_LEN {
            return Err(DomainError::FieldTooLong {
                field: "Saving name",
                max: Self::MAX_NAME_LEN,
            });
        }

        Ok(Self {
            id: None,
            name: trimmed.to_owned(),
            goal,
            current,
            date_range,
        })
    }

    pub fn with_id(mut self, id: i32) -> Self {
        self.id = Some(id);
        self
    }

    /// Remaining amount to save; zero once the goal is met or exceeded.
    pub fn remaining(&self) -> Result<Money, DomainError> {
        let remaining = self.goal.sub(&self.current)?;
        if remaining.is_negative() {
            return Ok(Money::zero(self.goal.currency.clone()));
        }
        Ok(remaining)
    }

    pub fn is_reached(&self) -> bool {
        !matches!(
            self.goal.partial_cmp(&self.current),
            Some(std::cmp::Ordering::Greater)
        )
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

    fn saving(goal: i64, current: i64) -> Saving {
        Saving::new("Holiday", pln(goal), pln(current), range()).unwrap()
    }

    #[test]
    fn remaining_counts_down_to_the_goal() {
        assert_eq!(saving(1000, 250).remaining().unwrap(), pln(750));
    }

    #[test]
    fn remaining_never_goes_negative() {
        assert_eq!(saving(1000, 1200).remaining().unwrap(), pln(0));
    }

    #[test]
    fn a_met_goal_is_reached() {
        assert!(saving(1000, 1000).is_reached());
        assert!(saving(1000, 1200).is_reached());
        assert!(!saving(1000, 999).is_reached());
    }

    #[test]
    fn mixed_currencies_are_rejected() {
        assert!(Saving::new("Holiday", pln(100), money(50, "USD"), range()).is_err());
    }

    #[test]
    fn a_blank_name_is_rejected() {
        assert!(Saving::new("  ", pln(100), pln(0), range()).is_err());
    }

    #[test]
    fn negative_amounts_are_rejected() {
        assert!(Saving::new("Holiday", pln(-1), pln(0), range()).is_err());
        assert!(Saving::new("Holiday", pln(100), pln(-1), range()).is_err());
    }
}
