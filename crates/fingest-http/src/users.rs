use std::collections::HashMap;

use actix_web::{HttpResponse, web};
use fingest_contracts::UserDto;
use fingest_identity_core::{Account, UserService};

use crate::{error::ApiError, extractor::AuthenticatedUser};

fn to_dto(account: Account) -> UserDto {
    UserDto {
        login: account.login,
        first_name: account.first_name,
        last_name: account.last_name,
        admin: account.admin,
    }
}

/// Lists every account.
///
/// v1 required both that the path login match the caller *and* that the caller be an
/// admin; that pair is preserved rather than simplified, because relaxing it would widen
/// access.
pub async fn get_users(
    user: AuthenticatedUser,
    service: web::Data<UserService>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let login = path.into_inner();
    user.require_self_or_admin(&login)?;
    user.require_admin()?;

    let accounts = service.list().await?;
    let body: Vec<UserDto> = accounts.into_iter().map(to_dto).collect();

    Ok(HttpResponse::Ok().json(body))
}

pub async fn update_user(
    user: AuthenticatedUser,
    service: web::Data<UserService>,
    path: web::Path<String>,
    query: web::Query<HashMap<String, String>>,
    body: web::Json<HashMap<String, serde_json::Value>>,
) -> Result<HttpResponse, ApiError> {
    let login = path.into_inner();
    user.require_self_or_admin(&login)?;

    let field = query
        .get("field")
        .ok_or_else(|| ApiError::BadRequest("Field parameter is required".to_owned()))?;

    // v1 reads the new value from the body key of the same name as the query parameter.
    let value = body.get(field).and_then(serde_json::Value::as_str);

    service.update_name(&login, field, value).await?;

    Ok(HttpResponse::NoContent().finish())
}

pub async fn delete_user(
    user: AuthenticatedUser,
    service: web::Data<UserService>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let login = path.into_inner();
    user.require_self_or_admin(&login)?;

    service.delete(&login).await?;

    Ok(HttpResponse::NoContent().finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{json_config::json_config_plain, resources::user_routes};
    use actix_web::{App, http::StatusCode, test};
    use fingest_contracts::ErrorResponse;
    use fingest_identity_core::testing::{FakeTokens, InMemoryAccountRepository, auth_service};
    use fingest_identity_core::{Account, AccountRepository, TokenVerifier};
    use std::sync::Arc;

    const ADMIN: (&str, &str) = ("Authorization", "Bearer token-for-root-admin=true");
    const BOB: (&str, &str) = ("Authorization", "Bearer token-for-bob-admin=false");

    async fn seeded_repo() -> Arc<InMemoryAccountRepository> {
        let repo = Arc::new(InMemoryAccountRepository::new());
        for (login, admin) in [("root", true), ("bob", false), ("alice", false)] {
            let account = Account::new(login, Some("First".into()), Some("Last".into()), admin)
                .expect("valid fixture");
            repo.insert(&account, "hash").await.unwrap();
        }
        repo
    }

    macro_rules! app_with {
        ($repo:expr) => {{
            let repo = $repo;
            let verifier: Arc<dyn TokenVerifier> = Arc::new(FakeTokens);
            test::init_service(
                App::new()
                    .app_data(web::Data::new(UserService::new(repo.clone())))
                    .app_data(web::Data::new(auth_service(repo)))
                    .app_data(json_config_plain())
                    .configure(|cfg| user_routes(cfg, verifier.clone())),
            )
            .await
        }};
    }

    // --- GET (admin only) ---

    #[actix_web::test]
    async fn admin_can_list_accounts() {
        let app = app_with!(seeded_repo().await);

        let req = test::TestRequest::get()
            .insert_header(ADMIN)
            .uri("/resources/users/root")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Vec<UserDto> = test::read_body_json(resp).await;
        assert_eq!(body.len(), 3);
    }

    #[actix_web::test]
    async fn listing_never_exposes_a_password() {
        let app = app_with!(seeded_repo().await);

        let req = test::TestRequest::get()
            .insert_header(ADMIN)
            .uri("/resources/users/root")
            .to_request();
        let body: serde_json::Value =
            test::read_body_json(test::call_service(&app, req).await).await;

        for entry in body.as_array().unwrap() {
            assert!(entry.get("password").is_none());
            assert_eq!(entry.as_object().unwrap().len(), 4);
        }
    }

    #[actix_web::test]
    async fn a_non_admin_cannot_list_accounts() {
        let app = app_with!(seeded_repo().await);

        let req = test::TestRequest::get()
            .insert_header(BOB)
            .uri("/resources/users/bob")
            .to_request();

        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::FORBIDDEN
        );
    }

    #[actix_web::test]
    async fn listing_without_a_token_is_401() {
        let app = app_with!(seeded_repo().await);

        let req = test::TestRequest::get()
            .uri("/resources/users/root")
            .to_request();

        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }

    // --- PUT ---

    #[actix_web::test]
    async fn a_user_can_update_their_own_name() {
        let repo = seeded_repo().await;
        let app = app_with!(repo.clone());

        let req = test::TestRequest::put()
            .insert_header(BOB)
            .uri("/resources/users/bob?field=firstName")
            .set_json(serde_json::json!({"firstName": "Robert"}))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let stored = repo.find("bob").await.unwrap().unwrap();
        assert_eq!(stored.account.first_name.as_deref(), Some("Robert"));
    }

    #[actix_web::test]
    async fn a_user_cannot_update_someone_else() {
        let repo = seeded_repo().await;
        let app = app_with!(repo.clone());

        let req = test::TestRequest::put()
            .insert_header(BOB)
            .uri("/resources/users/alice?field=firstName")
            .set_json(serde_json::json!({"firstName": "Hacked"}))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let stored = repo.find("alice").await.unwrap().unwrap();
        assert_eq!(stored.account.first_name.as_deref(), Some("First"));
    }

    #[actix_web::test]
    async fn a_missing_field_parameter_is_400() {
        let app = app_with!(seeded_repo().await);

        let req = test::TestRequest::put()
            .insert_header(BOB)
            .uri("/resources/users/bob")
            .set_json(serde_json::json!({"firstName": "Robert"}))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.message, "Field parameter is required");
    }

    #[actix_web::test]
    async fn the_admin_flag_cannot_be_updated() {
        let repo = seeded_repo().await;
        let app = app_with!(repo.clone());

        let req = test::TestRequest::put()
            .insert_header(BOB)
            .uri("/resources/users/bob?field=admin")
            .set_json(serde_json::json!({"admin": true}))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            !repo.find("bob").await.unwrap().unwrap().account.admin,
            "privilege escalation must be impossible through this route"
        );
    }

    // --- DELETE ---

    #[actix_web::test]
    async fn a_user_can_delete_their_own_account() {
        let repo = seeded_repo().await;
        let app = app_with!(repo.clone());

        let req = test::TestRequest::delete()
            .insert_header(BOB)
            .uri("/resources/users/bob")
            .to_request();

        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::NO_CONTENT
        );
        assert!(repo.find("bob").await.unwrap().is_none());
    }

    #[actix_web::test]
    async fn a_user_cannot_delete_someone_else() {
        let repo = seeded_repo().await;
        let app = app_with!(repo.clone());

        let req = test::TestRequest::delete()
            .insert_header(BOB)
            .uri("/resources/users/alice")
            .to_request();

        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::FORBIDDEN
        );
        assert!(repo.find("alice").await.unwrap().is_some());
    }

    #[actix_web::test]
    async fn an_admin_can_delete_anyone() {
        let repo = seeded_repo().await;
        let app = app_with!(repo.clone());

        let req = test::TestRequest::delete()
            .insert_header(ADMIN)
            .uri("/resources/users/alice")
            .to_request();

        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::NO_CONTENT
        );
        assert!(repo.find("alice").await.unwrap().is_none());
    }

    #[actix_web::test]
    async fn deleting_an_unknown_account_is_404() {
        let app = app_with!(seeded_repo().await);

        let req = test::TestRequest::delete()
            .insert_header(ADMIN)
            .uri("/resources/users/ghost")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.message, "User with login 'ghost' not found");
    }

    // --- stale tokens ---

    #[actix_web::test]
    async fn a_token_for_a_deleted_account_is_401() {
        let repo = seeded_repo().await;
        let app = app_with!(repo.clone());

        let delete = test::TestRequest::delete()
            .insert_header(ADMIN)
            .uri("/resources/users/alice")
            .to_request();
        assert_eq!(
            test::call_service(&app, delete).await.status(),
            StatusCode::NO_CONTENT
        );

        let req = test::TestRequest::put()
            .insert_header(("Authorization", "Bearer token-for-alice-admin=false"))
            .uri("/resources/users/alice?field=firstName")
            .set_json(serde_json::json!({"firstName": "Back"}))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.message, "Account no longer exists");
    }

    /// The token still says admin, the account no longer is.
    #[actix_web::test]
    async fn a_stale_admin_claim_does_not_grant_admin() {
        let app = app_with!(seeded_repo().await);

        let req = test::TestRequest::get()
            .insert_header(("Authorization", "Bearer token-for-bob-admin=true"))
            .uri("/resources/users/bob")
            .to_request();

        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::FORBIDDEN
        );
    }
}
