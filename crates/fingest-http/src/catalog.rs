use actix_web::{HttpResponse, web};
use fingest_catalog_core::{Category, CategoryService};
use fingest_contracts::{CategoryDto, CreateCategoryRequest, UpdateCategoryRequest};

use crate::{error::ApiError, extractor::AuthenticatedUser};

/// Mapping lives here, not in `fingest-contracts`: the contracts crate must not depend on a
/// bounded context, or the wire format would follow the domain around.
fn to_dto(category: Category) -> CategoryDto {
    CategoryDto {
        name: category.name,
        profit: category.profit,
    }
}

pub async fn list_categories(
    service: web::Data<CategoryService>,
) -> Result<HttpResponse, ApiError> {
    let categories = service.list().await?;
    let body: Vec<CategoryDto> = categories.into_iter().map(to_dto).collect();
    Ok(HttpResponse::Ok().json(body))
}

pub async fn create_category(
    user: AuthenticatedUser,
    service: web::Data<CategoryService>,
    body: web::Json<CreateCategoryRequest>,
) -> Result<HttpResponse, ApiError> {
    user.require_admin()?;
    let created = service.create(&body.name, body.profit).await?;
    Ok(HttpResponse::Created().json(to_dto(created)))
}

pub async fn update_category(
    user: AuthenticatedUser,
    service: web::Data<CategoryService>,
    path: web::Path<(String, bool)>,
    body: web::Json<UpdateCategoryRequest>,
) -> Result<HttpResponse, ApiError> {
    user.require_admin()?;
    let (name, profit) = path.into_inner();
    let updated = service.rename(&name, profit, &body.new_name).await?;
    Ok(HttpResponse::Ok().json(to_dto(updated)))
}

pub async fn delete_category(
    user: AuthenticatedUser,
    service: web::Data<CategoryService>,
    path: web::Path<(String, bool)>,
) -> Result<HttpResponse, ApiError> {
    user.require_admin()?;
    let (name, profit) = path.into_inner();
    service.delete(&name, profit).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Listing is public reference data; mutating shared categories requires an admin.
pub fn category_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/resources/categories")
            .route("", web::get().to(list_categories))
            .route("", web::post().to(create_category))
            .route("/{name}/{profit}", web::put().to(update_category))
            .route("/{name}/{profit}", web::delete().to(delete_category)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{extractor::TokenVerifierRef, json_config::json_config_plain};
    use actix_web::{App, http::StatusCode, test};
    use fingest_catalog_core::testing::InMemoryCategoryRepository;
    use fingest_contracts::ErrorResponse;
    use fingest_identity_core::TokenVerifier;
    use fingest_identity_core::testing::{FakeTokens, InMemoryAccountRepository, auth_service};
    use std::sync::Arc;

    const ADMIN: (&str, &str) = ("Authorization", "Bearer token-for-root-admin=true");
    const NON_ADMIN: (&str, &str) = ("Authorization", "Bearer token-for-bob-admin=false");

    /// Builds the real route table over an in-memory repository — no database, no mocks.
    macro_rules! app_with {
        ($repo:expr) => {{
            let verifier: Arc<dyn TokenVerifier> = Arc::new(FakeTokens);
            let accounts = Arc::new(InMemoryAccountRepository::with_accounts(&[
                ("root", true),
                ("bob", false),
            ]));
            test::init_service(
                App::new()
                    .app_data(web::Data::new(CategoryService::new($repo)))
                    .app_data(web::Data::new(TokenVerifierRef(verifier)))
                    .app_data(web::Data::new(auth_service(accounts)))
                    .app_data(json_config_plain())
                    .configure(category_routes),
            )
            .await
        }};
    }

    fn category(name: &str, profit: bool) -> Category {
        Category::new(name, profit).expect("valid fixture")
    }

    #[actix_web::test]
    async fn get_returns_200_and_the_category_list() {
        let repo = Arc::new(InMemoryCategoryRepository::seeded([
            category("Food", false),
            category("Salary", true),
        ]));
        let app = app_with!(repo);

        let req = test::TestRequest::get()
            .uri("/resources/categories")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Vec<CategoryDto> = test::read_body_json(resp).await;
        assert_eq!(body.len(), 2);
        assert_eq!(body[0].name, "Food");
        assert!(!body[0].profit);
    }

    #[actix_web::test]
    async fn post_returns_201_and_persists() {
        let repo = Arc::new(InMemoryCategoryRepository::new());
        let app = app_with!(repo.clone());

        let req = test::TestRequest::post()
            .insert_header(ADMIN)
            .uri("/resources/categories")
            .set_json(CreateCategoryRequest {
                name: "Food".into(),
                profit: false,
            })
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::CREATED);
        assert!(repo.contains("Food", false));
    }

    #[actix_web::test]
    async fn post_duplicate_returns_409_with_the_v1_message() {
        let repo = Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )]));
        let app = app_with!(repo);

        let req = test::TestRequest::post()
            .insert_header(ADMIN)
            .uri("/resources/categories")
            .set_json(CreateCategoryRequest {
                name: "FOOD".into(),
                profit: false,
            })
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.status, "409");
        assert_eq!(
            body.message,
            "Category 'FOOD' with profit=false already exists"
        );
    }

    #[actix_web::test]
    async fn put_renames_and_returns_200() {
        let repo = Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )]));
        let app = app_with!(repo.clone());

        let req = test::TestRequest::put()
            .insert_header(ADMIN)
            .uri("/resources/categories/Food/false")
            .set_json(UpdateCategoryRequest {
                new_name: "Groceries".into(),
            })
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body: CategoryDto = test::read_body_json(resp).await;
        assert_eq!(body.name, "Groceries");
        assert!(repo.contains("Groceries", false));
    }

    #[actix_web::test]
    async fn put_missing_returns_404() {
        let repo = Arc::new(InMemoryCategoryRepository::new());
        let app = app_with!(repo);

        let req = test::TestRequest::put()
            .insert_header(ADMIN)
            .uri("/resources/categories/Food/false")
            .set_json(UpdateCategoryRequest {
                new_name: "Groceries".into(),
            })
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.message, "Category 'Food' with profit=false not found");
    }

    #[actix_web::test]
    async fn delete_returns_204_and_removes() {
        let repo = Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )]));
        let app = app_with!(repo.clone());

        let req = test::TestRequest::delete()
            .insert_header(ADMIN)
            .uri("/resources/categories/Food/false")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(repo.is_empty());
    }

    #[actix_web::test]
    async fn delete_missing_returns_404() {
        let repo = Arc::new(InMemoryCategoryRepository::new());
        let app = app_with!(repo);

        let req = test::TestRequest::delete()
            .insert_header(ADMIN)
            .uri("/resources/categories/Food/false")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn profit_flag_selects_the_right_category() {
        let repo = Arc::new(InMemoryCategoryRepository::seeded([
            category("Food", false),
            category("Food", true),
        ]));
        let app = app_with!(repo.clone());

        let req = test::TestRequest::delete()
            .insert_header(ADMIN)
            .uri("/resources/categories/Food/true")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(repo.contains("Food", false), "expense variant must survive");
        assert!(!repo.contains("Food", true));
    }

    #[actix_web::test]
    async fn malformed_json_returns_the_v1_400_body() {
        let repo = Arc::new(InMemoryCategoryRepository::new());
        let app = app_with!(repo);

        let req = test::TestRequest::post()
            .insert_header(ADMIN)
            .uri("/resources/categories")
            .insert_header(("content-type", "application/json"))
            .set_payload(r#"{"name":"Food","profit":"not-a-bool"}"#)
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.status, "400");
    }

    // --- authorization (D1/D2) ---

    #[actix_web::test]
    async fn listing_stays_public() {
        let app = app_with!(Arc::new(InMemoryCategoryRepository::new()));

        let req = test::TestRequest::get()
            .uri("/resources/categories")
            .to_request();

        assert_eq!(test::call_service(&app, req).await.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn mutations_without_a_token_are_401() {
        let repo = Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )]));
        let app = app_with!(repo.clone());

        let cases = [
            test::TestRequest::post()
                .uri("/resources/categories")
                .set_json(CreateCategoryRequest {
                    name: "Zebra".into(),
                    profit: false,
                })
                .to_request(),
            test::TestRequest::put()
                .uri("/resources/categories/Food/false")
                .set_json(UpdateCategoryRequest {
                    new_name: "Groceries".into(),
                })
                .to_request(),
            test::TestRequest::delete()
                .uri("/resources/categories/Food/false")
                .to_request(),
        ];

        for req in cases {
            let resp = test::call_service(&app, req).await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        assert!(repo.contains("Food", false), "nothing may have changed");
    }

    #[actix_web::test]
    async fn mutations_by_a_non_admin_are_403() {
        let repo = Arc::new(InMemoryCategoryRepository::seeded([category(
            "Food", false,
        )]));
        let app = app_with!(repo.clone());

        let req = test::TestRequest::delete()
            .insert_header(NON_ADMIN)
            .uri("/resources/categories/Food/false")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(repo.contains("Food", false), "the row must survive");
    }

    #[actix_web::test]
    async fn an_invalid_token_is_401_not_403() {
        let app = app_with!(Arc::new(InMemoryCategoryRepository::new()));

        let req = test::TestRequest::post()
            .insert_header(("Authorization", "Bearer nonsense"))
            .uri("/resources/categories")
            .set_json(CreateCategoryRequest {
                name: "Zebra".into(),
                profit: false,
            })
            .to_request();

        assert_eq!(
            test::call_service(&app, req).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
}
