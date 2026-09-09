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
