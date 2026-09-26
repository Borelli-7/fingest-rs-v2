use std::collections::HashMap;

use actix_web::{HttpResponse, web};
use fingest_contracts::{
    ExpenseDto, ExpenseInputRequest, SummaryDto, UpdateExpenseRequest, UpdateWalletRequest,
    WalletDto,
};
use fingest_kernel::DateRange;
use fingest_wallets_core::{
    Expense, ExpensePatch, NewExpense, Summary, Wallet, WalletPatch, WalletService,
};

use crate::{error::ApiError, extractor::AuthenticatedUser};

fn wallet_dto(wallet: Wallet) -> WalletDto {
    WalletDto {
        id: wallet.id,
        name: wallet.name,
        amount: wallet.amount,
    }
}

fn expense_dto(expense: Expense) -> ExpenseDto {
    ExpenseDto {
        id: expense.id,
        amount: expense.amount,
        date: expense.date,
        description: expense.description,
        category: expense.category,
    }
}

fn summary_dto(summary: Summary) -> SummaryDto {
    SummaryDto {
        wallet_name: summary.wallet_name,
        balance: summary.balance,
        expense_categories: summary.expense_categories,
        income_categories: summary.income_categories,
        total_expense: summary.total_expense,
        total_income: summary.total_income,
    }
}

/// v1 read `start` and `end` from the query string, widening absent bounds to the sentinels.
fn date_range(query: &HashMap<String, String>) -> Result<DateRange, ApiError> {
    DateRange::from_string(
        query.get("start").map(String::as_str),
        query.get("end").map(String::as_str),
    )
    .map_err(|e| ApiError::BadRequest(format!("Invalid date format: {e}")))
}

// --- wallets ---

pub async fn get_wallets(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let login = path.into_inner();
    user.require_self_or_admin(&login)?;

    let wallets = service.list_wallets(&login).await?;
    let body: Vec<WalletDto> = wallets.into_iter().map(wallet_dto).collect();

    Ok(HttpResponse::Ok().json(body))
}

pub async fn create_wallet(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<String>,
    body: web::Json<WalletDto>,
) -> Result<HttpResponse, ApiError> {
    let login = path.into_inner();
    user.require_self_or_admin(&login)?;

    let body = body.into_inner();
    let created = service
        .create_wallet(&login, body.name, body.amount)
        .await?;
    let location = format!(
        "/resources/users/{login}/wallets/{}",
        created.id.unwrap_or_default()
    );

    Ok(HttpResponse::Created()
        .append_header(("Location", location))
        .json(wallet_dto(created)))
}

pub async fn update_wallet(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32)>,
    body: web::Json<UpdateWalletRequest>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let body = body.into_inner();
    let updated = service
        .update_wallet(
            &login,
            wallet_id,
            WalletPatch {
                name: body.name,
                amount: body.amount,
            },
        )
        .await?;

    Ok(HttpResponse::Ok().json(wallet_dto(updated)))
}

pub async fn delete_wallet(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32)>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    service.delete_wallet(&login, wallet_id).await?;

    Ok(HttpResponse::NoContent().finish())
}

// --- reads ---

pub async fn get_summary(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32)>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let summary = service
        .summary(&login, wallet_id, &date_range(&query)?)
        .await?;

    Ok(HttpResponse::Ok().json(summary_dto(summary)))
}

pub async fn get_expenses(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32)>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let expenses = service
        .list_expenses(&login, wallet_id, &date_range(&query)?)
        .await?;
    let body: Vec<ExpenseDto> = expenses.into_iter().map(expense_dto).collect();

    Ok(HttpResponse::Ok().json(body))
}

pub async fn get_highest_expense(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32)>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let highest = service
        .highest_expense(&login, wallet_id, &date_range(&query)?)
        .await?;

    Ok(HttpResponse::Ok().json(highest.map(expense_dto)))
}

pub async fn get_counted_categories(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32)>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let counts = service
        .counted_categories(&login, wallet_id, &date_range(&query)?)
        .await?;

    Ok(HttpResponse::Ok().json(counts))
}

// --- expenses ---

pub async fn create_expense(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32)>,
    body: web::Json<ExpenseInputRequest>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let body = body.into_inner();
    let created = service
        .record_expense(
            &login,
            wallet_id,
            NewExpense {
                amount: body.amount,
                date: body.date,
                description: body.description,
                category: body.category,
            },
        )
        .await?;

    // v1 omits the `/resources` prefix here; kept for wire parity.
    let location = format!(
        "/{login}/wallets/{wallet_id}/expenses/{}",
        created.id.unwrap_or_default()
    );

    Ok(HttpResponse::Created()
        .append_header(("Location", location))
        .json(expense_dto(created)))
}

pub async fn update_expense(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32, i32)>,
    body: web::Json<UpdateExpenseRequest>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id, expense_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let body = body.into_inner();
    let updated = service
        .update_expense(
            &login,
            wallet_id,
            expense_id,
            ExpensePatch {
                amount: body.amount,
                date: body.date,
                description: body.description,
                category: body.category,
            },
        )
        .await?;

    Ok(HttpResponse::Ok().json(expense_dto(updated)))
}

pub async fn delete_expense(
    user: AuthenticatedUser,
    service: web::Data<WalletService>,
    path: web::Path<(String, i32, i32)>,
) -> Result<HttpResponse, ApiError> {
    let (login, wallet_id, expense_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    service
        .delete_expense(&login, wallet_id, expense_id)
        .await?;

    Ok(HttpResponse::NoContent().finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{json_config::json_config_plain, resources::user_routes};
    use actix_web::{App, http::StatusCode, test};
    use bigdecimal::BigDecimal;
    use chrono::NaiveDate;
    use fingest_identity_core::{
        TokenVerifier, UserService,
        testing::{FakeTokens, InMemoryAccountRepository, auth_service, fixed_clock},
    };
    use fingest_kernel::{CategoryRef, Currency, Money, SystemClock};
    use fingest_wallets_core::{
        UnitOfWork, WalletReader,
        testing::{InMemoryStore, InMemoryUnitOfWork},
    };
    use serde_json::{Value, json};
    use std::sync::Arc;

    const OWNER: &str = "Bearer token-for-bob-admin=false";
    const STRANGER: &str = "Bearer token-for-mallory-admin=false";
    const ADMIN: &str = "Bearer token-for-root-admin=true";

    fn pln(n: i64) -> Money {
        Money::new(BigDecimal::from(n), Currency::default())
    }

    /// bob owns wallet 1 (100 PLN, one 30 PLN Food entry as expense 1); mallory owns wallet 2.
    fn store() -> Arc<InMemoryStore> {
        let lunch = Expense::new(
            pln(30),
            NaiveDate::from_ymd_opt(2024, 6, 15).unwrap(),
            "lunch",
            CategoryRef::new("Food", false).unwrap(),
        )
        .unwrap();

        Arc::new(
            InMemoryStore::new()
                .with_owner("bob")
                .with_owner("mallory")
                .with_owner("root")
                .with_category("Food", false)
                .with_category("Salary", true)
                .with_wallet("bob", Wallet::new("Main", pln(100)).unwrap())
                .with_wallet("mallory", Wallet::new("Other", pln(5)).unwrap())
                .with_expense(1, lunch),
        )
    }

    async fn send(
        store: Arc<InMemoryStore>,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, actix_web::http::header::HeaderMap, Value) {
        let verifier: Arc<dyn TokenVerifier> = Arc::new(FakeTokens);
        let service = WalletService::new(
            Arc::clone(&store) as Arc<dyn WalletReader>,
            Arc::new(InMemoryUnitOfWork::new(store)) as Arc<dyn UnitOfWork>,
            Arc::new(SystemClock),
        );
        // The extractor re-checks every token against the stored account.
        let accounts = Arc::new(InMemoryAccountRepository::with_accounts(&[
            ("bob", false),
            ("mallory", false),
            ("root", true),
        ]));
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(service))
                .app_data(web::Data::new(UserService::new(
                    accounts.clone(),
                    fixed_clock(),
                )))
                .app_data(web::Data::new(auth_service(accounts)))
                .app_data(json_config_plain())
                .configure(|cfg| user_routes(cfg, verifier)),
        )
        .await;

        let mut req = match method {
            "GET" => test::TestRequest::get(),
            "POST" => test::TestRequest::post(),
            "PUT" => test::TestRequest::put(),
            "DELETE" => test::TestRequest::delete(),
            other => panic!("unsupported method {other}"),
        }
        .uri(uri);
        if let Some(token) = token {
            req = req.insert_header(("Authorization", token));
        }
        if let Some(body) = body {
            req = req.set_json(body);
        }

        let resp = test::call_service(&app, req.to_request()).await;
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = test::read_body(resp).await;
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, json)
    }

    fn pln_json(amount: &str) -> Value {
        json!({"amount": amount, "currency": "PLN"})
    }

    /// Every wallet and expense route, with a body that succeeds for its owner.
    fn routes() -> Vec<(&'static str, &'static str, Option<Value>, StatusCode)> {
        let base = "/resources/users/bob/wallets";
        let entry = json!({
            "amount": pln_json("10"),
            "date": "2024-06-16",
            "description": "bus",
            "category": {"name": "Food", "profit": false}
        });
        vec![
            ("GET", base, None, StatusCode::OK),
            (
                "POST",
                base,
                Some(json!({"name": "Savings", "amount": pln_json("50")})),
                StatusCode::CREATED,
            ),
            (
                "PUT",
                "/resources/users/bob/wallets/1",
                Some(json!({"name": "Daily"})),
                StatusCode::OK,
            ),
            (
                "DELETE",
                "/resources/users/bob/wallets/1",
                None,
                StatusCode::NO_CONTENT,
            ),
            (
                "GET",
                "/resources/users/bob/wallets/1/summary",
                None,
                StatusCode::OK,
            ),
            (
                "GET",
                "/resources/users/bob/wallets/1/highest_expense",
                None,
                StatusCode::OK,
            ),
            (
                "GET",
                "/resources/users/bob/wallets/1/counted_categories",
                None,
                StatusCode::OK,
            ),
            (
                "GET",
                "/resources/users/bob/wallets/1/expenses",
                None,
                StatusCode::OK,
            ),
            (
                "POST",
                "/resources/users/bob/wallets/1/expenses",
                Some(entry),
                StatusCode::CREATED,
            ),
            (
                "PUT",
                "/resources/users/bob/wallets/1/expenses/1",
                Some(json!({"description": "renamed"})),
                StatusCode::OK,
            ),
            (
                "DELETE",
                "/resources/users/bob/wallets/1/expenses/1",
                None,
                StatusCode::NO_CONTENT,
            ),
        ]
    }

    #[actix_web::test]
    async fn the_owner_can_use_every_route() {
        for (method, uri, body, expected) in routes() {
            let (status, _, json) = send(store(), method, uri, Some(OWNER), body).await;
            assert_eq!(status, expected, "{method} {uri}: {json}");
        }
    }

    #[actix_web::test]
    async fn an_admin_can_act_on_another_users_wallets() {
        for (method, uri, body, expected) in routes() {
            let (status, _, json) = send(store(), method, uri, Some(ADMIN), body).await;
            assert_eq!(status, expected, "{method} {uri}: {json}");
        }
    }

    #[actix_web::test]
    async fn another_user_is_forbidden_and_nothing_changes() {
        for (method, uri, body, _) in routes() {
            let store = store();
            let (status, _, json) =
                send(Arc::clone(&store), method, uri, Some(STRANGER), body).await;

            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
            assert_eq!(json["status"], "403");
            assert_eq!(store.balance_of(1), Some(pln(100)), "{method} {uri}");
            assert_eq!(store.wallet_count(), 2, "{method} {uri}");
            assert_eq!(store.expense_count(), 1, "{method} {uri}");
        }
    }

    #[actix_web::test]
    async fn every_route_requires_a_token() {
        for (method, uri, body, _) in routes() {
            let (status, _, _) = send(store(), method, uri, None, body).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
    }

    #[actix_web::test]
    async fn create_wallet_returns_the_location_of_the_new_wallet() {
        let (status, headers, json) = send(
            store(),
            "POST",
            "/resources/users/bob/wallets",
            Some(OWNER),
            Some(json!({"name": "Savings", "amount": pln_json("50")})),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            headers.get("Location").unwrap(),
            "/resources/users/bob/wallets/3"
        );
        assert_eq!(json["name"], "Savings");
    }

    #[actix_web::test]
    async fn recording_an_expense_moves_the_balance_and_uses_the_v1_location() {
        let store = store();
        let (status, headers, _) = send(
            Arc::clone(&store),
            "POST",
            "/resources/users/bob/wallets/1/expenses",
            Some(OWNER),
            Some(json!({
                "amount": pln_json("10"),
                "date": "2024-06-16",
                "description": "bus",
                "category": {"name": "Food", "profit": false}
            })),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            headers.get("Location").unwrap(),
            "/bob/wallets/1/expenses/2"
        );
        assert_eq!(store.balance_of(1), Some(pln(90)));
    }

    /// D8.
    #[actix_web::test]
    async fn an_expense_in_another_currency_is_400() {
        let store = store();
        let (status, _, json) = send(
            Arc::clone(&store),
            "POST",
            "/resources/users/bob/wallets/1/expenses",
            Some(OWNER),
            Some(json!({
                "amount": {"amount": "10", "currency": "USD"},
                "date": "2024-06-16",
                "description": "bus",
                "category": {"name": "Food", "profit": false}
            })),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["message"], "Currency mismatch: expected PLN, got USD");
        assert_eq!(store.expense_count(), 1);
    }

    #[actix_web::test]
    async fn a_malformed_amount_on_a_money_route_gets_the_v1_amount_message() {
        let (status, _, json) = send(
            store(),
            "POST",
            "/resources/users/bob/wallets",
            Some(OWNER),
            Some(json!({"name": "Savings", "amount": {"amount": "abc", "currency": "PLN"}})),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["message"], "The amount is invalid");
    }

    /// D13: a route with no money in its body must not claim the amount is invalid.
    #[actix_web::test]
    async fn malformed_json_on_a_non_money_route_is_invalid_request_data() {
        let (status, _, json) = send(
            store(),
            "PUT",
            "/resources/users/bob?field=firstName",
            Some(OWNER),
            Some(json!(["not", "an", "object"])),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["message"], "Invalid request data");
    }

    #[actix_web::test]
    async fn a_malformed_date_filter_is_400() {
        let (status, _, json) = send(
            store(),
            "GET",
            "/resources/users/bob/wallets/1/expenses?start=15-06-2024",
            Some(OWNER),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .starts_with("Invalid date format")
        );
    }

    #[actix_web::test]
    async fn the_summary_reports_totals_for_the_range() {
        let (status, _, json) = send(
            store(),
            "GET",
            "/resources/users/bob/wallets/1/summary?start=2024-01-01&end=2024-12-31",
            Some(OWNER),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["wallet_name"], "Main");
        assert_eq!(json["expense_categories"]["Food"]["amount"], "30");
    }

    #[actix_web::test]
    async fn a_wallet_that_does_not_exist_is_404() {
        let (status, _, json) = send(
            store(),
            "PUT",
            "/resources/users/bob/wallets/99",
            Some(OWNER),
            Some(json!({"name": "Ghost"})),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["message"], "Wallet with id 99 not found");
    }
}
