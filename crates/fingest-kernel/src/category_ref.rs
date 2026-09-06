use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// A reference to a category owned by the catalog context.
///
/// Identity is the composite `(name, profit)`, matching the database primary key. This is
/// deliberately *not* the catalog aggregate: other contexts point at categories, they do
/// not own or validate their lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CategoryRef {
    pub name: String,
    pub profit: bool,
}

impl CategoryRef {
    pub fn new(name: impl Into<String>, profit: bool) -> Result<Self, DomainError> {
        let name = name.into();
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err(DomainError::EmptyField {
                field: "Category name",
            });
        }

        Ok(Self {
            name: trimmed.to_owned(),
            profit,
        })
    }

    /// True when the category increases a balance rather than reducing it.
    pub fn is_income(&self) -> bool {
        self.profit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_includes_the_profit_flag() {
        let expense = CategoryRef::new("Food", false).unwrap();
        let income = CategoryRef::new("Food", true).unwrap();

        assert_ne!(expense, income);
        assert!(!expense.is_income());
        assert!(income.is_income());
    }

    #[test]
    fn a_blank_name_is_rejected() {
        assert!(CategoryRef::new("  ", false).is_err());
    }

    #[test]
    fn wire_shape_is_name_and_profit() {
        let json = serde_json::to_value(CategoryRef::new("Food", false).unwrap()).unwrap();
        assert_eq!(json["name"], "Food");
        assert_eq!(json["profit"], false);
        assert_eq!(json.as_object().unwrap().len(), 2);
    }
}
