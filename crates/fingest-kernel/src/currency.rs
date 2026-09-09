use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// ISO-4217-shaped currency code: exactly three ASCII letters, normalised to uppercase.
///
/// Serialises as a bare string, so the wire format is unchanged from v1 (`"PLN"`).
/// Deserialisation validates, which is deviation D9.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Currency(String);

impl Currency {
    /// v1 stored `VARCHAR(3)` and defaulted to PLN when the client omitted a currency.
    pub const DEFAULT: &'static str = "PLN";

    pub fn new(code: impl AsRef<str>) -> Result<Self, DomainError> {
        let code = code.as_ref().trim();
        if code.len() != 3 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(DomainError::InvalidCurrencyCode(code.to_owned()));
        }
        Ok(Self(code.to_ascii_uppercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Currency {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Currency {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Currency> for String {
    fn from(value: Currency) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_pln() {
        assert_eq!(Currency::default().as_str(), "PLN");
    }

    #[test]
    fn accepts_three_letters_and_uppercases() {
        assert_eq!(Currency::new("usd").unwrap().as_str(), "USD");
        assert_eq!(Currency::new("EUR").unwrap().as_str(), "EUR");
    }

    #[test]
    fn rejects_wrong_length_or_non_alpha() {
        for bad in ["", "US", "USDD", "US1", "12345"] {
            assert!(Currency::new(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn serialises_as_bare_string() {
        let json = serde_json::to_string(&Currency::new("PLN").unwrap()).unwrap();
        assert_eq!(json, "\"PLN\"");
    }

    #[test]
    fn deserialise_validates() {
        assert!(serde_json::from_str::<Currency>("\"PLN\"").is_ok());
        assert!(serde_json::from_str::<Currency>("\"PLNX\"").is_err());
    }
}
