use fingest_kernel::{DomainError, PortError};
use thiserror::Error;

/// Use-case level failures for the catalog context.
///
/// Variants map 1:1 onto v1's `AppError` cases so `fingest-http` can reproduce the legacy
/// status codes and message strings exactly.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    Internal(String),

    #[error(transparent)]
    Port(#[from] PortError),
}

impl From<DomainError> for CatalogError {
    fn from(err: DomainError) -> Self {
        Self::Validation(err.to_string())
    }
}

impl CatalogError {
    /// v1: `"Category '{name}' with profit={profit} not found"`
    pub fn not_found(name: &str, profit: bool) -> Self {
        Self::NotFound(format!("Category '{name}' with profit={profit} not found"))
    }

    /// v1: `"Category '{name}' with profit={profit} already exists"`
    pub fn already_exists(name: &str, profit: bool) -> Self {
        Self::Conflict(format!(
            "Category '{name}' with profit={profit} already exists"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_match_v1_wording() {
        assert_eq!(
            CatalogError::not_found("Food", false).to_string(),
            "Category 'Food' with profit=false not found"
        );
        assert_eq!(
            CatalogError::already_exists("Food", true).to_string(),
            "Category 'Food' with profit=true already exists"
        );
    }

    #[test]
    fn domain_errors_become_validation_failures() {
        let err: CatalogError = DomainError::EmptyField {
            field: "Category name",
        }
        .into();
        assert_eq!(
            err,
            CatalogError::Validation("Category name cannot be empty".into())
        );
    }
}
