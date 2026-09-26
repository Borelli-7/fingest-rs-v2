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

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, http::StatusCode, test};
    use fingest_kernel::Money;

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
