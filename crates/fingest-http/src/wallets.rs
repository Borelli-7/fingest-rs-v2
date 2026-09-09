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
