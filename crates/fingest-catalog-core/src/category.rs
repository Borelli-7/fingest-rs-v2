use fingest_kernel::DomainError;
use serde::{Deserialize, Serialize};

/// An expense or income category.
///
/// Identity is the composite `(name, profit)` — v1's primary key — so "Food" as an expense
/// and "Food" as income are two distinct categories.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub profit: bool,
}

impl Category {
    pub const MAX_NAME_LEN: usize = 255;

    pub fn new(name: impl Into<String>, profit: bool) -> Result<Self, DomainError> {
        let name = name.into();
        let trimmed = name.trim();

        if trimmed.is_empty() {
            return Err(DomainError::EmptyField {
                field: "Category name",
            });
        }
        if trimmed.chars().count() > Self::MAX_NAME_LEN {
            return Err(DomainError::FieldTooLong {
                field: "Category name",
                max: Self::MAX_NAME_LEN,
            });
        }

        Ok(Self {
            name: trimmed.to_owned(),
            profit,
        })
    }

    /// v1 treats names case-insensitively when detecting duplicates.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.profit == other.profit && self.name.eq_ignore_ascii_case(&other.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_or_whitespace_name() {
        assert_eq!(
            Category::new("", false).unwrap_err(),
            DomainError::EmptyField {
                field: "Category name"
            }
        );
        assert!(Category::new("   ", false).is_err());
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(Category::new("  Food  ", false).unwrap().name, "Food");
    }

    #[test]
    fn rejects_overlong_name() {
        let long = "x".repeat(Category::MAX_NAME_LEN + 1);
        assert_eq!(
            Category::new(long, false).unwrap_err(),
            DomainError::FieldTooLong {
                field: "Category name",
                max: 255
            }
        );
        assert!(Category::new("x".repeat(Category::MAX_NAME_LEN), false).is_ok());
    }

    #[test]
    fn identity_includes_profit_flag() {
        let expense = Category::new("Food", false).unwrap();
        let income = Category::new("Food", true).unwrap();
        assert_ne!(expense, income);
        assert!(!expense.conflicts_with(&income));
    }

    #[test]
    fn conflict_detection_ignores_case() {
        let a = Category::new("Food", false).unwrap();
        let b = Category::new("FOOD", false).unwrap();
        assert!(a.conflicts_with(&b));
    }
}
