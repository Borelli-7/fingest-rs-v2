use std::cmp::Ordering;
use std::sync::LazyLock;

use bigdecimal::BigDecimal;
use serde::{Deserialize, Serialize};

use crate::{currency::Currency, error::DomainError};

/// Shared zero, so sign checks do not allocate a `BigDecimal` per call.
static ZERO: LazyLock<BigDecimal> = LazyLock::new(|| BigDecimal::from(0));

/// A quantity of money in a single currency.
///
/// Sign is deliberately unconstrained: a wallet balance may legitimately go negative when
/// expenses exceed deposits. Non-negativity is an *input* rule, enforced by callers via
/// [`Money::require_non_negative`] at the same boundaries v1 applied `#[validate]`
/// (wallet creation and expense input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Money {
    pub amount: BigDecimal,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: BigDecimal, currency: Currency) -> Self {
        Self { amount, currency }
    }

    pub fn zero(currency: Currency) -> Self {
        Self {
            amount: ZERO.clone(),
            currency,
        }
    }

    /// Parses a decimal string, defaulting the currency to PLN when absent, mirroring v1.
    pub fn parse(amount: &str, currency: Option<&str>) -> Result<Self, DomainError> {
        let parsed: BigDecimal = amount
            .parse()
            .map_err(|_| DomainError::InvalidAmount(amount.to_owned()))?;
        let currency = match currency {
            Some(c) => Currency::new(c)?,
            None => Currency::default(),
        };
        Ok(Self::new(parsed, currency))
    }

    pub fn is_negative(&self) -> bool {
        self.amount < *ZERO
    }

    pub fn require_non_negative(&self) -> Result<(), DomainError> {
        if self.is_negative() {
            return Err(DomainError::NegativeAmount);
        }
        Ok(())
    }

    fn require_same_currency(&self, other: &Self) -> Result<(), DomainError> {
        if self.currency != other.currency {
            return Err(DomainError::CurrencyMismatch {
                expected: self.currency.to_string(),
                actual: other.currency.to_string(),
            });
        }
        Ok(())
    }

    pub fn add(&self, other: &Self) -> Result<Self, DomainError> {
        self.require_same_currency(other)?;
        Ok(Self::new(
            &self.amount + &other.amount,
            self.currency.clone(),
        ))
    }

    pub fn sub(&self, other: &Self) -> Result<Self, DomainError> {
        self.require_same_currency(other)?;
        Ok(Self::new(
            &self.amount - &other.amount,
            self.currency.clone(),
        ))
    }

    pub fn negate(&self) -> Self {
        Self::new(-self.amount.clone(), self.currency.clone())
    }
}

/// Equal only when both currency and amount match — v1 semantics.
impl PartialEq for Money {
    fn eq(&self, other: &Self) -> bool {
        self.currency == other.currency && self.amount == other.amount
    }
}

/// Uncomparable across currencies — v1 semantics.
impl PartialOrd for Money {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.currency != other.currency {
            None
        } else {
            self.amount.partial_cmp(&other.amount)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pln(n: i64) -> Money {
        Money::new(BigDecimal::from(n), Currency::default())
    }

    fn usd(n: i64) -> Money {
        Money::new(BigDecimal::from(n), Currency::new("USD").unwrap())
    }

    // --- ported from v1 src/models/money.rs ---

    #[test]
    fn new_with_currency() {
        let money = usd(100);
        assert_eq!(money.amount, BigDecimal::from(100));
        assert_eq!(money.currency.as_str(), "USD");
    }

    #[test]
    fn parse_defaults_currency_to_pln() {
        let money = Money::parse("99.99", None).unwrap();
        assert_eq!(money.currency.as_str(), "PLN");
    }

    #[test]
    fn parse_with_currency() {
        let money = Money::parse("123.45", Some("EUR")).unwrap();
        assert_eq!(money.amount, "123.45".parse::<BigDecimal>().unwrap());
        assert_eq!(money.currency.as_str(), "EUR");
    }

    #[test]
    fn parse_rejects_non_numeric() {
        assert_eq!(
            Money::parse("abc", None).unwrap_err(),
            DomainError::InvalidAmount("abc".into())
        );
    }

    #[test]
    fn zero_is_zero() {
        assert_eq!(Money::zero(Currency::default()).amount, BigDecimal::from(0));
    }

    // --- new invariants ---

    #[test]
    fn equality_requires_matching_currency() {
        assert_eq!(pln(100), pln(100));
        assert_ne!(pln(100), usd(100));
    }

    #[test]
    fn ordering_is_none_across_currencies() {
        assert_eq!(pln(100).partial_cmp(&usd(100)), None);
        assert_eq!(pln(50).partial_cmp(&pln(100)), Some(Ordering::Less));
    }

    #[test]
    fn add_and_sub_same_currency() {
        assert_eq!(pln(30).add(&pln(12)).unwrap(), pln(42));
        assert_eq!(pln(30).sub(&pln(12)).unwrap(), pln(18));
    }

    #[test]
    fn add_across_currencies_is_rejected() {
        let err = pln(30).add(&usd(12)).unwrap_err();
        assert_eq!(
            err,
            DomainError::CurrencyMismatch {
                expected: "PLN".into(),
                actual: "USD".into()
            }
        );
    }

    #[test]
    fn balance_may_go_negative() {
        let balance = pln(10).sub(&pln(25)).unwrap();
        assert!(balance.is_negative());
        assert_eq!(balance.amount, BigDecimal::from(-15));
    }

    #[test]
    fn require_non_negative_guards_input_amounts() {
        assert!(pln(1).require_non_negative().is_ok());
        assert!(pln(0).require_non_negative().is_ok());
        assert_eq!(
            pln(-1).require_non_negative().unwrap_err(),
            DomainError::NegativeAmount
        );
    }

    #[test]
    fn wire_shape_matches_v1() {
        let json = serde_json::to_value(pln(50)).unwrap();
        assert_eq!(json["currency"], "PLN");
        assert!(json.get("amount").is_some());
    }
}
