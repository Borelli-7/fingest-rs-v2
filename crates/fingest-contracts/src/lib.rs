//! Frozen wire contracts.
//!
//! Every shape here is public API reproduced from v1. Renaming a field is a breaking
//! change; the tests in this crate pin the JSON key names so that cannot happen silently.

use std::collections::HashMap;

use chrono::NaiveDate;
use fingest_kernel::{CategoryRef, DateRange, Money};
use serde::{Deserialize, Serialize};

/// v1's error body: `{"status":"404","message":"..."}`. Note `status` is a *string*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub status: String,
    pub message: String,
}

impl ErrorResponse {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status: status.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryDto {
    pub name: String,
    pub profit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateCategoryRequest {
    pub name: String,
    pub profit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCategoryRequest {
    pub new_name: String,
}

/// v1 renames these two fields; everything else is snake_case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserDto {
    pub login: String,
    #[serde(rename = "firstName")]
    pub first_name: Option<String>,
    #[serde(rename = "lastName")]
    pub last_name: Option<String>,
    pub admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub login: String,
    #[serde(rename = "firstName")]
    pub first_name: Option<String>,
    #[serde(rename = "lastName")]
    pub last_name: Option<String>,
    pub password: String,
    pub admin: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenValidationResponse {
    pub valid: bool,
    pub login: String,
    pub admin: bool,
}

/// Used for both the wallet response and the create request, as in v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalletDto {
    pub id: Option<i32>,
    pub name: String,
    pub amount: Money,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateWalletRequest {
    pub name: Option<String>,
    pub amount: Option<Money>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpenseDto {
    pub id: Option<i32>,
    pub amount: Money,
    pub date: NaiveDate,
    pub description: String,
    pub category: CategoryRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpenseInputRequest {
    pub amount: Money,
    pub date: NaiveDate,
    pub description: String,
    pub category: CategoryRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateExpenseRequest {
    pub amount: Option<Money>,
    pub date: Option<NaiveDate>,
    pub description: Option<String>,
    pub category: Option<CategoryRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryDto {
    pub wallet_name: String,
    pub balance: Money,
    pub expense_categories: HashMap<String, Money>,
    pub income_categories: HashMap<String, Money>,
    pub total_expense: Money,
    pub total_income: Money,
}

/// Budget responses use `date_range`; requests use `dateRange`. That asymmetry is v1's,
/// and the tests below pin it so it cannot be "tidied up" by accident.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetDto {
    pub id: Option<i32>,
    pub category: CategoryRef,
    pub total: Money,
    pub date_range: DateRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetOutputDto {
    pub id: Option<i32>,
    pub category: CategoryRef,
    pub total: Money,
    pub date_range: DateRange,
    pub spent: Money,
    pub left: Money,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetInputRequest {
    pub category: CategoryRef,
    pub total: Money,
    #[serde(rename = "dateRange")]
    pub date_range: DateRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateBudgetRequest {
    pub category: Option<CategoryRef>,
    pub total: Option<Money>,
    #[serde(rename = "dateRange")]
    pub date_range: Option<DateRange>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_body_matches_v1_shape() {
        let json = serde_json::to_value(ErrorResponse::new(404, "Not found")).unwrap();
        assert_eq!(json["status"], "404", "status is a string in v1");
        assert_eq!(json["message"], "Not found");
        assert_eq!(json.as_object().unwrap().len(), 2, "no extra fields");
    }

    #[test]
    fn category_keys_are_name_and_profit() {
        let json = serde_json::to_value(CategoryDto {
            name: "Food".into(),
            profit: false,
        })
        .unwrap();
        assert_eq!(json["name"], "Food");
        assert_eq!(json["profit"], false);
        assert_eq!(json.as_object().unwrap().len(), 2);
    }

    #[test]
    fn update_request_key_is_snake_case_new_name() {
        let parsed: UpdateCategoryRequest =
            serde_json::from_str(r#"{"new_name":"Groceries"}"#).unwrap();
        assert_eq!(parsed.new_name, "Groceries");
    }

    #[test]
    fn user_dto_uses_camel_case_name_fields() {
        let json = serde_json::to_value(UserDto {
            login: "bob".into(),
            first_name: Some("Bob".into()),
            last_name: Some("Builder".into()),
            admin: false,
        })
        .unwrap();

        assert_eq!(json["firstName"], "Bob");
        assert_eq!(json["lastName"], "Builder");
        assert!(json.get("first_name").is_none());
    }

    #[test]
    fn user_dto_can_never_carry_a_password() {
        let json = serde_json::to_value(UserDto {
            login: "bob".into(),
            first_name: None,
            last_name: None,
            admin: false,
        })
        .unwrap();

        assert_eq!(json.as_object().unwrap().len(), 4);
        assert!(json.get("password").is_none());
    }

    #[test]
    fn register_request_accepts_the_v1_payload() {
        let parsed: RegisterRequest = serde_json::from_str(
            r#"{"login":"bob","firstName":"Bob","lastName":"B","password":"secret123","admin":false}"#,
        )
        .unwrap();

        assert_eq!(parsed.login, "bob");
        assert_eq!(parsed.first_name.as_deref(), Some("Bob"));
        assert_eq!(parsed.admin, Some(false));
    }

    #[test]
    fn register_request_allows_omitted_optional_fields() {
        let parsed: RegisterRequest =
            serde_json::from_str(r#"{"login":"bob","password":"secret123"}"#).unwrap();

        assert_eq!(parsed.first_name, None);
        assert_eq!(parsed.admin, None);
    }

    #[test]
    fn verify_response_matches_v1_shape() {
        let json = serde_json::to_value(TokenValidationResponse {
            valid: true,
            login: "bob".into(),
            admin: true,
        })
        .unwrap();

        assert_eq!(json["valid"], true);
        assert_eq!(json["login"], "bob");
        assert_eq!(json["admin"], true);
    }

    fn a_range() -> DateRange {
        DateRange::new(
            Some(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
            Some(chrono::NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
        )
    }

    fn a_category() -> CategoryRef {
        CategoryRef::new("Food", false).unwrap()
    }

    fn a_money() -> Money {
        Money::parse("500.00", Some("PLN")).unwrap()
    }

    /// v1 accepts `dateRange` on the way in...
    #[test]
    fn budget_requests_use_camel_case_date_range() {
        let parsed: BudgetInputRequest = serde_json::from_str(
            r#"{"category":{"name":"Food","profit":false},
                "total":{"amount":"500.00","currency":"PLN"},
                "dateRange":{"start":"2024-01-01","end":"2024-12-31"}}"#,
        )
        .unwrap();

        assert_eq!(parsed.date_range.start.to_string(), "2024-01-01");
    }

    /// ...and emits `date_range` on the way out. Asymmetric, but it is the contract.
    #[test]
    fn budget_responses_use_snake_case_date_range() {
        let json = serde_json::to_value(BudgetDto {
            id: Some(1),
            category: a_category(),
            total: a_money(),
            date_range: a_range(),
        })
        .unwrap();

        assert!(json.get("date_range").is_some());
        assert!(json.get("dateRange").is_none());
    }

    #[test]
    fn budget_output_carries_spent_and_left() {
        let json = serde_json::to_value(BudgetOutputDto {
            id: Some(1),
            category: a_category(),
            total: a_money(),
            date_range: a_range(),
            spent: a_money(),
            left: a_money(),
        })
        .unwrap();

        assert!(json.get("spent").is_some());
        assert!(json.get("left").is_some());
        assert_eq!(json.as_object().unwrap().len(), 6);
    }

    #[test]
    fn budget_update_allows_omitted_fields() {
        let parsed: UpdateBudgetRequest = serde_json::from_str(r#"{}"#).unwrap();

        assert!(parsed.category.is_none());
        assert!(parsed.total.is_none());
        assert!(parsed.date_range.is_none());
    }
}
