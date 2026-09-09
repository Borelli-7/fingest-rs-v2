use fingest_kernel::{DomainError, PortError};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlanningError {
    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Forbidden(String),

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Validation(String),

    #[error(transparent)]
    Port(#[from] PortError),
}

impl From<DomainError> for PlanningError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::CurrencyMismatch { .. } => Self::BadRequest(err.to_string()),
            other => Self::Validation(other.to_string()),
        }
    }
}

impl PlanningError {
    /// v1: `"User with login '{login}' not found"`
    pub fn user_not_found(login: &str) -> Self {
        Self::NotFound(format!("User with login '{login}' not found"))
    }

    /// v1: `"Budget with id {id} not found"`
    pub fn budget_not_found(budget_id: i32) -> Self {
        Self::NotFound(format!("Budget with id {budget_id} not found"))
    }

    /// v1: `"Category {name} with profit={profit} does not exist"`, returned as 400.
    pub fn unknown_category(name: &str, profit: bool) -> Self {
        Self::BadRequest(format!(
            "Category {name} with profit={profit} does not exist"
        ))
    }

    pub fn not_owner(action: &str) -> Self {
        Self::Forbidden(format!("Not authorized to {action} this budget"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_match_v1_wording() {
        assert_eq!(
            PlanningError::budget_not_found(7).to_string(),
            "Budget with id 7 not found"
        );
        assert_eq!(
            PlanningError::not_owner("update").to_string(),
            "Not authorized to update this budget"
        );
        assert_eq!(
            PlanningError::unknown_category("Food", false).to_string(),
            "Category Food with profit=false does not exist"
        );
    }
}
