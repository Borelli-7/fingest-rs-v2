use actix_web::{HttpRequest, HttpResponse, web};
use fingest_contracts::{
    LoginRequest, LoginResponse, RegisterRequest, TokenValidationResponse, UserDto,
};
use fingest_identity_core::{Account, AuthService, NewAccount};

use crate::{error::ApiError, extractor::bearer_token, rate_limit::RateLimiter};

fn to_dto(account: Account) -> UserDto {
    UserDto {
        login: account.login,
        first_name: account.first_name,
        last_name: account.last_name,
        admin: account.admin,
    }
}

/// Consumes one attempt per key; the first exhausted key refuses the request.
fn throttle(limiter: &RateLimiter, keys: &[String]) -> Result<(), ApiError> {
    for key in keys {
        limiter.check(key).map_err(|wait| {
            ApiError::TooManyRequests(
                "Too many attempts. Try again later.".to_owned(),
                wait.as_secs().max(1),
            )
        })?;
    }
    Ok(())
}

/// The socket peer, never `X-Forwarded-For`: a client-supplied header would let an
/// attacker pick a fresh key for every request.
fn peer_key(req: &HttpRequest) -> String {
    let ip = req
        .peer_addr()
        .map_or_else(|| "unknown".to_owned(), |addr| addr.ip().to_string());
    format!("ip:{ip}")
}

pub async fn register(
    service: web::Data<AuthService>,
    limiter: web::Data<RateLimiter>,
    req: HttpRequest,
    body: web::Json<RegisterRequest>,
) -> Result<HttpResponse, ApiError> {
    throttle(&limiter, &[peer_key(&req)])?;
    let body = body.into_inner();
    let created = service
        .register(NewAccount {
            login: body.login,
            first_name: body.first_name,
            last_name: body.last_name,
            password: body.password,
            admin: body.admin.unwrap_or(false),
        })
        .await?;

    Ok(HttpResponse::Created().json(to_dto(created)))
}

pub async fn login(
    service: web::Data<AuthService>,
    limiter: web::Data<RateLimiter>,
    req: HttpRequest,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, ApiError> {
    // Per client and per account, so neither one address nor a botnet can guess freely.
    throttle(&limiter, &[peer_key(&req), format!("login:{}", body.login)])?;

    let (token, account) = service.login(&body.login, &body.password).await?;

    Ok(HttpResponse::Ok().json(LoginResponse {
        token,
        user: to_dto(account),
    }))
}

/// Reads the header directly rather than going through the extractor, because v1 reports a
/// distinct message here.
pub async fn verify_token(
    service: web::Data<AuthService>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let token = bearer_token(&req).ok_or_else(|| {
        ApiError::Unauthenticated("Missing or invalid Authorization header".to_owned())
    })?;

    let claims = service.verify_token(token)?;

    Ok(HttpResponse::Ok().json(TokenValidationResponse {
        valid: true,
        login: claims.sub,
        admin: claims.admin,
    }))
}

pub fn auth_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/auth")
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/verify", web::get().to(verify_token)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json_config::json_config_plain;
    use actix_web::{App, http::StatusCode, test};
    use fingest_contracts::ErrorResponse;
    use fingest_identity_core::testing::{CountingHasher, FakeTokens, InMemoryAccountRepository};
    use std::sync::Arc;

    fn auth_service(repo: Arc<InMemoryAccountRepository>) -> AuthService {
        let tokens = Arc::new(FakeTokens);
        AuthService::new(
            repo,
            Arc::new(CountingHasher::new()),
            tokens.clone(),
            tokens,
        )
    }

    macro_rules! app_with {
        ($repo:expr) => {
            app_with!(
                $repo,
                RateLimiter::new(1_000, std::time::Duration::from_secs(60))
            )
        };
        ($repo:expr, $limiter:expr) => {
            test::init_service(
                App::new()
                    .app_data(web::Data::new(auth_service($repo)))
                    .app_data(web::Data::new($limiter))
                    .app_data(json_config_plain())
                    .configure(auth_routes),
            )
            .await
        };
    }

    fn register_body(login: &str, password: &str) -> serde_json::Value {
        serde_json::json!({ "login": login, "password": password })
    }

    #[actix_web::test]
    async fn register_returns_201_and_never_echoes_the_password() {
        let app = app_with!(Arc::new(InMemoryAccountRepository::new()));

        let req = test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(register_body("bob", "correct-horse"))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["login"], "bob");
        assert!(body.get("password").is_none());
        assert!(body.get("password_hash").is_none());
    }

    #[actix_web::test]
    async fn register_rejects_a_short_password_with_400() {
        let app = app_with!(Arc::new(InMemoryAccountRepository::new()));

        let req = test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(register_body("bob", "short"))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn register_rejects_a_duplicate_with_400_like_v1() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let app = app_with!(repo);

        for _ in 0..2 {
            let req = test::TestRequest::post()
                .uri("/api/auth/register")
                .set_json(register_body("bob", "correct-horse"))
                .to_request();
            let resp = test::call_service(&app, req).await;
            if resp.status() == StatusCode::BAD_REQUEST {
                let body: ErrorResponse = test::read_body_json(resp).await;
                assert_eq!(body.message, "User with login 'bob' already exists");
                return;
            }
        }
        panic!("second registration should have been rejected");
    }

    #[actix_web::test]
    async fn login_returns_a_token() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let app = app_with!(repo);

        let req = test::TestRequest::post()
            .uri("/api/auth/register")
            .set_json(register_body("bob", "correct-horse"))
            .to_request();
        test::call_service(&app, req).await;

        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(serde_json::json!({"login":"bob","password":"correct-horse"}))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body: LoginResponse = test::read_body_json(resp).await;
        assert!(!body.token.is_empty());
        assert_eq!(body.user.login, "bob");
    }

    #[actix_web::test]
    async fn login_with_bad_credentials_is_401() {
        let repo = Arc::new(InMemoryAccountRepository::new());
        let app = app_with!(repo);

        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(serde_json::json!({"login":"nobody","password":"whatever"}))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.message, "Invalid credentials");
    }

    #[actix_web::test]
    async fn repeated_logins_are_throttled_with_429() {
        let app = app_with!(
            Arc::new(InMemoryAccountRepository::new()),
            RateLimiter::new(2, std::time::Duration::from_secs(60))
        );

        let attempt = || {
            test::TestRequest::post()
                .uri("/api/auth/login")
                .set_json(serde_json::json!({"login":"bob","password":"guess-guess"}))
                .to_request()
        };

        for _ in 0..2 {
            assert_eq!(
                test::call_service(&app, attempt()).await.status(),
                StatusCode::UNAUTHORIZED
            );
        }

        let resp = test::call_service(&app, attempt()).await;
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(resp.headers().contains_key("Retry-After"));
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.status, "429");
        assert_eq!(body.message, "Too many attempts. Try again later.");
    }

    #[actix_web::test]
    async fn registration_is_throttled_too() {
        let app = app_with!(
            Arc::new(InMemoryAccountRepository::new()),
            RateLimiter::new(1, std::time::Duration::from_secs(60))
        );

        for (login, expected) in [
            ("first", StatusCode::CREATED),
            ("second", StatusCode::TOO_MANY_REQUESTS),
        ] {
            let req = test::TestRequest::post()
                .uri("/api/auth/register")
                .set_json(register_body(login, "correct-horse"))
                .to_request();
            assert_eq!(test::call_service(&app, req).await.status(), expected);
        }
    }

    #[actix_web::test]
    async fn verify_without_a_header_is_401() {
        let app = app_with!(Arc::new(InMemoryAccountRepository::new()));

        let req = test::TestRequest::get()
            .uri("/api/auth/verify")
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: ErrorResponse = test::read_body_json(resp).await;
        assert_eq!(body.message, "Missing or invalid Authorization header");
    }

    #[actix_web::test]
    async fn verify_reports_the_claims() {
        let app = app_with!(Arc::new(InMemoryAccountRepository::new()));

        let req = test::TestRequest::get()
            .uri("/api/auth/verify")
            .insert_header(("Authorization", "Bearer token-for-root-admin=true"))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);
        let body: TokenValidationResponse = test::read_body_json(resp).await;
        assert!(body.valid);
        assert_eq!(body.login, "root");
        assert!(body.admin);
    }
}
