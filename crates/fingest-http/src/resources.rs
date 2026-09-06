use std::sync::Arc;

use actix_web::web;
use fingest_identity_core::TokenVerifier;

use crate::{budgets, json_config::json_config_money, middleware::JwtAuth, users, wallets};

/// Everything under `/resources/users`, in a single scope.
///
/// One scope rather than several: actix matches a scope by prefix, so two scopes sharing
/// `/resources/users` would let the first one answer 404 for paths only the second defines.
///
/// Every route here is protected, so the scope carries [`JwtAuth`]; the `AuthenticatedUser`
/// extractor then reuses the claims it deposited rather than verifying twice.
pub fn user_routes(cfg: &mut web::ServiceConfig, verifier: Arc<dyn TokenVerifier>) {
    cfg.service(
        web::scope("/resources/users")
            .wrap(JwtAuth::new(verifier))
            // accounts
            .service(
                web::resource("/{login}")
                    .route(web::get().to(users::get_users))
                    .route(web::put().to(users::update_user))
                    .route(web::delete().to(users::delete_user)),
            )
            // wallets — money in the body, so the amount-aware JSON error applies
            .service(
                web::resource("/{login}/wallets")
                    .app_data(json_config_money())
                    .route(web::get().to(wallets::get_wallets))
                    .route(web::post().to(wallets::create_wallet)),
            )
            .service(
                web::resource("/{login}/wallets/{id}")
                    .app_data(json_config_money())
                    .route(web::put().to(wallets::update_wallet))
                    .route(web::delete().to(wallets::delete_wallet)),
            )
            .service(
                web::resource("/{login}/wallets/{id}/summary")
                    .route(web::get().to(wallets::get_summary)),
            )
            .service(
                web::resource("/{login}/wallets/{id}/highest_expense")
                    .route(web::get().to(wallets::get_highest_expense)),
            )
            .service(
                web::resource("/{login}/wallets/{id}/counted_categories")
                    .route(web::get().to(wallets::get_counted_categories)),
            )
            // expenses
            .service(
                web::resource("/{login}/wallets/{id}/expenses")
                    .app_data(json_config_money())
                    .route(web::get().to(wallets::get_expenses))
                    .route(web::post().to(wallets::create_expense)),
            )
            .service(
                web::resource("/{login}/wallets/{id}/expenses/{expense_id}")
                    .app_data(json_config_money())
                    .route(web::put().to(wallets::update_expense))
                    .route(web::delete().to(wallets::delete_expense)),
            )
            // budgets
            .service(
                web::resource("/{login}/budgets")
                    .app_data(json_config_money())
                    .route(web::get().to(budgets::get_budgets))
                    .route(web::post().to(budgets::create_budget)),
            )
            .service(
                web::resource("/{login}/budgets/{budget_id}")
                    .app_data(json_config_money())
                    .route(web::put().to(budgets::update_budget))
                    .route(web::delete().to(budgets::delete_budget)),
            ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test};
    use fingest_identity_core::testing::FakeTokens;

    const TOKEN: (&str, &str) = ("Authorization", "Bearer token-for-bob-admin=false");

    /// Savings are modelled and persisted but must not be reachable — decision 3. Without
    /// this, adding a `Saving` handler somewhere would silently widen the API.
    #[actix_web::test]
    async fn savings_are_not_exposed() {
        let verifier: Arc<dyn TokenVerifier> = Arc::new(FakeTokens);
        let app =
            test::init_service(App::new().configure(|cfg| user_routes(cfg, verifier.clone())))
                .await;

        for path in [
            "/resources/users/bob/savings",
            "/resources/users/bob/savings/1",
        ] {
            let req = test::TestRequest::get()
                .insert_header(TOKEN)
                .uri(path)
                .to_request();

            assert_eq!(
                test::call_service(&app, req).await.status(),
                StatusCode::NOT_FOUND,
                "{path} must not be routed"
            );
        }
    }

    #[actix_web::test]
    async fn every_protected_path_rejects_a_missing_token() {
        let verifier: Arc<dyn TokenVerifier> = Arc::new(FakeTokens);
        let app =
            test::init_service(App::new().configure(|cfg| user_routes(cfg, verifier.clone())))
                .await;

        for path in [
            "/resources/users/bob",
            "/resources/users/bob/wallets",
            "/resources/users/bob/wallets/1/expenses",
            "/resources/users/bob/budgets",
        ] {
            let req = test::TestRequest::get().uri(path).to_request();

            assert_eq!(
                test::call_service(&app, req).await.status(),
                StatusCode::UNAUTHORIZED,
                "{path} must require a token"
            );
        }
    }
}
