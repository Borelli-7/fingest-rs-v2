use actix_web::{HttpResponse, error::InternalError, web};
use fingest_contracts::ErrorResponse;

/// For payloads carrying a `Money` field.
///
/// v1 sniffed the serde message for `invalid digit` / `invalid type` / `expected` and
/// reported "The amount is invalid". Clients depend on that exact string, so it is kept —
/// but only where an amount actually exists.
pub fn json_config_money() -> web::JsonConfig {
    web::JsonConfig::default().error_handler(|err, _req| {
        let detail = err.to_string();
        let message = if detail.contains("invalid digit")
            || detail.contains("invalid type")
            || detail.contains("expected")
            || detail.contains("Invalid amount")
        {
            "The amount is invalid"
        } else {
            "Invalid request data"
        };

        InternalError::from_response(
            err,
            HttpResponse::BadRequest().json(ErrorResponse::new(400, message)),
        )
        .into()
    })
}

/// Deviation D13: applying v1's amount heuristic to a payload with no amount produced
/// "The amount is invalid" for, say, a malformed category. Those routes now say what they
/// mean. The status code is unchanged.
pub fn json_config_plain() -> web::JsonConfig {
    web::JsonConfig::default().error_handler(|err, _req| {
        InternalError::from_response(
            err,
            HttpResponse::BadRequest().json(ErrorResponse::new(400, "Invalid request data")),
        )
        .into()
    })
}

/// A path segment that does not parse (`/wallets/abc`) keeps actix's 404 — no such
/// resource — but with the `{"status","message"}` body every other error uses, rather
/// than actix's plain-text default.
pub fn path_config() -> web::PathConfig {
    web::PathConfig::default().error_handler(|err, _req| {
        InternalError::from_response(
            err,
            HttpResponse::NotFound().json(ErrorResponse::new(404, "Invalid path parameter")),
        )
        .into()
    })
}

pub fn query_config() -> web::QueryConfig {
    web::QueryConfig::default().error_handler(|err, _req| {
        InternalError::from_response(
            err,
            HttpResponse::BadRequest().json(ErrorResponse::new(400, "Invalid query parameters")),
        )
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test};
    use fingest_kernel::Money;

    async fn by_id(path: web::Path<(String, i32)>) -> HttpResponse {
        HttpResponse::Ok().body(path.1.to_string())
    }

    async fn by_flag(path: web::Path<(String, bool)>) -> HttpResponse {
        HttpResponse::Ok().body(path.1.to_string())
    }

    #[derive(serde::Deserialize)]
    struct Page {
        page: u32,
    }

    async fn paged(query: web::Query<Page>) -> HttpResponse {
        HttpResponse::Ok().body(query.page.to_string())
    }

    async fn call(uri: &str) -> (StatusCode, String, Option<ErrorResponse>) {
        let app = test::init_service(
            App::new()
                .app_data(path_config())
                .app_data(query_config())
                .route("/users/{login}/wallets/{id}", web::get().to(by_id))
                .route("/categories/{name}/{profit}", web::get().to(by_flag))
                .route("/paged", web::get().to(paged)),
        )
        .await;
        let resp = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        if status.is_success() {
            return (status, content_type, None);
        }
        (status, content_type, Some(test::read_body_json(resp).await))
    }

    #[actix_web::test]
    async fn a_non_integer_id_gets_the_json_error_shape() {
        let (status, content_type, body) = call("/users/bob/wallets/abc").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(content_type, "application/json");
        let body = body.unwrap();
        assert_eq!(body.status, "404");
        assert_eq!(body.message, "Invalid path parameter");
    }

    #[actix_web::test]
    async fn a_non_boolean_profit_flag_gets_the_json_error_shape() {
        let (status, _, body) = call("/categories/Food/maybe").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.unwrap().message, "Invalid path parameter");
    }

    #[actix_web::test]
    async fn a_malformed_query_is_400_with_the_json_error_shape() {
        let (status, _, body) = call("/paged?page=-1").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.unwrap().message, "Invalid query parameters");
    }

    #[actix_web::test]
    async fn well_formed_parameters_still_reach_the_handler() {
        assert_eq!(call("/users/bob/wallets/7").await.0, StatusCode::OK);
        assert_eq!(call("/categories/Food/false").await.0, StatusCode::OK);
        assert_eq!(call("/paged?page=2").await.0, StatusCode::OK);
    }

    async fn accept(_: web::Json<Money>) -> HttpResponse {
        HttpResponse::Ok().finish()
    }

    async fn post_amount(amount: &str) -> (StatusCode, Option<ErrorResponse>) {
        let app = test::init_service(
            App::new()
                .app_data(json_config_money())
                .route("/", web::post().to(accept)),
        )
        .await;
        let req = test::TestRequest::post()
            .uri("/")
            .set_json(serde_json::json!({"amount": amount, "currency": "PLN"}))
            .to_request();
        let resp = test::call_service(&app, req).await;
        let status = resp.status();
        if status.is_success() {
            return (status, None);
        }
        (status, Some(test::read_body_json(resp).await))
    }

    #[actix_web::test]
    async fn an_amount_postgres_would_round_is_400_with_the_v1_message() {
        let (status, body) = post_amount("10.005").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.unwrap().message, "The amount is invalid");
    }

    #[actix_web::test]
    async fn an_amount_that_would_overflow_is_400_not_500() {
        let (status, body) = post_amount("100000000000000000").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body.unwrap().message, "The amount is invalid");
    }

    #[actix_web::test]
    async fn a_storable_amount_is_accepted() {
        assert_eq!(post_amount("10.50").await.0, StatusCode::OK);
    }
}
