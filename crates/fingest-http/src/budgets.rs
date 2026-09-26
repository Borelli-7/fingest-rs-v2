use std::collections::HashMap;

use actix_web::{HttpResponse, web};
use fingest_contracts::{BudgetDto, BudgetInputRequest, BudgetOutputDto, UpdateBudgetRequest};
use fingest_kernel::DateRange;
use fingest_planning_core::{Budget, BudgetPatch, BudgetService, BudgetWithSpent};

use crate::{error::ApiError, extractor::AuthenticatedUser};

fn budget_dto(budget: Budget) -> BudgetDto {
    BudgetDto {
        id: budget.id,
        category: budget.category,
        total: budget.total,
        date_range: budget.date_range,
    }
}

fn output_dto(entry: BudgetWithSpent) -> Result<BudgetOutputDto, ApiError> {
    let left = entry
        .left()
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let BudgetWithSpent { budget, spent } = entry;

    Ok(BudgetOutputDto {
        id: budget.id,
        category: budget.category,
        total: budget.total,
        date_range: budget.date_range,
        spent,
        left,
    })
}

/// v1 filters budgets with two independent windows, each supplied as a `_min`/`_max` pair.
fn window(query: &HashMap<String, String>, min: &str, max: &str) -> Result<DateRange, ApiError> {
    DateRange::from_string(
        query.get(min).map(String::as_str),
        query.get(max).map(String::as_str),
    )
    .map_err(|e| ApiError::BadRequest(format!("Invalid date format: {e}")))
}

pub async fn get_budgets(
    user: AuthenticatedUser,
    service: web::Data<BudgetService>,
    path: web::Path<String>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, ApiError> {
    let login = path.into_inner();
    user.require_self_or_admin(&login)?;

    let budgets = service
        .list(
            &login,
            &window(&query, "start_min", "start_max")?,
            &window(&query, "end_min", "end_max")?,
        )
        .await?;

    let body: Vec<BudgetOutputDto> = budgets
        .into_iter()
        .map(output_dto)
        .collect::<Result<_, _>>()?;

    Ok(HttpResponse::Ok().json(body))
}

pub async fn create_budget(
    user: AuthenticatedUser,
    service: web::Data<BudgetService>,
    path: web::Path<String>,
    body: web::Json<BudgetInputRequest>,
) -> Result<HttpResponse, ApiError> {
    let login = path.into_inner();
    user.require_self_or_admin(&login)?;

    let body = body.into_inner();
    let created = service
        .create(&login, body.category, body.total, body.date_range)
        .await?;

    // v1 omits the `/resources` prefix here, as it does for expenses.
    let location = format!("/{login}/budgets/{}", created.id.unwrap_or_default());

    Ok(HttpResponse::Created()
        .append_header(("Location", location))
        .json(budget_dto(created)))
}

pub async fn update_budget(
    user: AuthenticatedUser,
    service: web::Data<BudgetService>,
    path: web::Path<(String, i32)>,
    body: web::Json<UpdateBudgetRequest>,
) -> Result<HttpResponse, ApiError> {
    let (login, budget_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    let body = body.into_inner();
    let updated = service
        .update(
            &login,
            budget_id,
            BudgetPatch {
                category: body.category,
                total: body.total,
                date_range: body.date_range,
            },
        )
        .await?;

    Ok(HttpResponse::Ok().json(budget_dto(updated)))
}

pub async fn delete_budget(
    user: AuthenticatedUser,
    service: web::Data<BudgetService>,
    path: web::Path<(String, i32)>,
) -> Result<HttpResponse, ApiError> {
    let (login, budget_id) = path.into_inner();
    user.require_self_or_admin(&login)?;

    service.delete(&login, budget_id).await?;

    Ok(HttpResponse::NoContent().finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::user_routes;
    use actix_web::{App, http::StatusCode, test};
    use chrono::NaiveDate;
    use fingest_identity_core::{
        TokenVerifier,
        testing::{FakeTokens, InMemoryAccountRepository, auth_service},
    };
    use fingest_kernel::{CategoryRef, Money, SystemClock};
    use fingest_planning_core::testing::InMemoryBudgetRepository;
    use serde_json::{Value, json};
    use std::sync::Arc;

    const OWNER: &str = "Bearer token-for-bob-admin=false";
    const STRANGER: &str = "Bearer token-for-mallory-admin=false";
    const ADMIN: &str = "Bearer token-for-root-admin=true";

    /// bob owns budget 1 (Food, 500 PLN, 2024).
    async fn repo() -> Arc<InMemoryBudgetRepository> {
        let repo = Arc::new(
            InMemoryBudgetRepository::new()
                .with_owner("bob")
                .with_owner("mallory")
                .with_category("Food", false),
        );
        BudgetService::new(Arc::clone(&repo) as _, Arc::new(SystemClock))
            .create(
                "bob",
                CategoryRef::new("Food", false).unwrap(),
                Money::parse("500", Some("PLN")).unwrap(),
                DateRange::new(
                    NaiveDate::from_ymd_opt(2024, 1, 1),
                    NaiveDate::from_ymd_opt(2024, 12, 31),
                ),
            )
            .await
            .unwrap();
        repo
    }

    async fn send(
        repo: Arc<InMemoryBudgetRepository>,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Option<String>, Value) {
        let verifier: Arc<dyn TokenVerifier> = Arc::new(FakeTokens);
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(auth_service(Arc::new(
                    InMemoryAccountRepository::with_accounts(&[
                        ("bob", false),
                        ("mallory", false),
                        ("root", true),
                    ]),
                ))))
                .app_data(web::Data::new(BudgetService::new(
                    repo,
                    Arc::new(SystemClock),
                )))
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
        let location = resp
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = test::read_body(resp).await;
        (
            status,
            location,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn new_budget() -> Value {
        json!({
            "category": {"name": "Food", "profit": false},
            "total": {"amount": "200", "currency": "PLN"},
            "dateRange": {"start": "2025-01-01", "end": "2025-12-31"}
        })
    }

    fn routes() -> Vec<(&'static str, &'static str, Option<Value>, StatusCode)> {
        vec![
            ("GET", "/resources/users/bob/budgets", None, StatusCode::OK),
            (
                "POST",
                "/resources/users/bob/budgets",
                Some(new_budget()),
                StatusCode::CREATED,
            ),
            (
                "PUT",
                "/resources/users/bob/budgets/1",
                Some(json!({"total": {"amount": "750", "currency": "PLN"}})),
                StatusCode::OK,
            ),
            (
                "DELETE",
                "/resources/users/bob/budgets/1",
                None,
                StatusCode::NO_CONTENT,
            ),
        ]
    }

    #[actix_web::test]
    async fn the_owner_can_use_every_route() {
        for (method, uri, body, expected) in routes() {
            let (status, _, json) = send(repo().await, method, uri, Some(OWNER), body).await;
            assert_eq!(status, expected, "{method} {uri}: {json}");
        }
    }

    /// D4: v1 denied admins here.
    #[actix_web::test]
    async fn an_admin_can_act_on_another_users_budgets() {
        for (method, uri, body, expected) in routes() {
            let (status, _, json) = send(repo().await, method, uri, Some(ADMIN), body).await;
            assert_eq!(status, expected, "{method} {uri}: {json}");
        }
    }

    #[actix_web::test]
    async fn another_user_is_forbidden_and_nothing_changes() {
        for (method, uri, body, _) in routes() {
            let repo = repo().await;
            let (status, _, _) = send(Arc::clone(&repo), method, uri, Some(STRANGER), body).await;

            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
            assert_eq!(repo.len(), 1, "{method} {uri}");
        }
    }

    #[actix_web::test]
    async fn every_route_requires_a_token() {
        for (method, uri, body, _) in routes() {
            let (status, _, _) = send(repo().await, method, uri, None, body).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        }
    }

    /// D7: the row is real and the id is not fabricated.
    #[actix_web::test]
    async fn create_persists_and_uses_the_v1_location() {
        let repo = repo().await;
        let (status, location, json) = send(
            Arc::clone(&repo),
            "POST",
            "/resources/users/bob/budgets",
            Some(OWNER),
            Some(new_budget()),
        )
        .await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(location.as_deref(), Some("/bob/budgets/2"));
        assert_eq!(json["id"], 2);
        assert!(json.get("date_range").is_some(), "responses use snake_case");
        assert_eq!(repo.len(), 2);
    }

    #[actix_web::test]
    async fn listing_reports_spent_and_left() {
        let (status, _, json) = send(
            repo().await,
            "GET",
            "/resources/users/bob/budgets",
            Some(OWNER),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json[0]["spent"]["amount"], "0");
        assert_eq!(json[0]["left"]["amount"], "500");
    }

    #[actix_web::test]
    async fn a_malformed_window_is_400() {
        let (status, _, json) = send(
            repo().await,
            "GET",
            "/resources/users/bob/budgets?start_min=yesterday",
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
    async fn an_unknown_budget_is_404() {
        let (status, _, json) = send(
            repo().await,
            "DELETE",
            "/resources/users/bob/budgets/99",
            Some(OWNER),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["status"], "404");
    }
}
