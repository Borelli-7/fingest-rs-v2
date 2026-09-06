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
