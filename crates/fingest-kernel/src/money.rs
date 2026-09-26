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
///
/// Deserialisation additionally enforces [`Money::require_storable`], so every amount that
/// arrives over the wire is one the `NUMERIC(19,2)` columns can hold exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "MoneyRepr")]
pub struct Money {
    pub amount: BigDecimal,
    pub currency: Currency,
}

#[derive(Deserialize)]
struct MoneyRepr {
    amount: BigDecimal,
    currency: Currency,
}

impl TryFrom<MoneyRepr> for Money {
    type Error = DomainError;

    fn try_from(repr: MoneyRepr) -> Result<Self, Self::Error> {
        let money = Self::new(repr.amount, repr.currency);
        money.require_storable()?;
        Ok(money)
    }
}

impl Money {
    /// Matches the `NUMERIC(19,2)` storage columns.
    pub const MAX_SCALE: i64 = 2;
    pub const MAX_INTEGER_DIGITS: u32 = 17;
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

    /// Rejects amounts Postgres would round (more than two decimals) or overflow. Without
    /// this the stored value silently differed from the one echoed back and published.
    pub fn require_storable(&self) -> Result<(), DomainError> {
        let normalized = self.amount.normalized();
        let limit = BigDecimal::from(10_i64.pow(Self::MAX_INTEGER_DIGITS));

        if normalized.fractional_digit_count() > Self::MAX_SCALE || normalized.abs() >= limit {
            return Err(DomainError::InvalidAmount(self.amount.to_string()));
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

    fn from_json(amount: &str) -> Result<Money, serde_json::Error> {
        serde_json::from_str(&format!(r#"{{"amount":"{amount}","currency":"PLN"}}"#))
    }

    #[test]
    fn two_decimal_places_are_accepted() {
        assert_eq!(
            from_json("10.55").unwrap().amount,
            "10.55".parse::<BigDecimal>().unwrap()
        );
        assert!(from_json("10.500").is_ok(), "trailing zeros lose nothing");
    }

    #[test]
    fn a_third_decimal_place_is_rejected_rather_than_rounded() {
        let err = from_json("10.005").unwrap_err();
        assert!(err.to_string().contains("Invalid amount"), "{err}");
    }

    #[test]
    fn amounts_that_overflow_numeric_19_2_are_rejected() {
        assert!(from_json("99999999999999999.99").is_ok());
        assert!(from_json("100000000000000000").is_err());
        assert!(from_json("-100000000000000000").is_err());
    }

    #[test]
    fn deserialisation_still_validates_the_currency() {
        assert!(serde_json::from_str::<Money>(r#"{"amount":"1","currency":"PLNX"}"#).is_err());
    }
}
