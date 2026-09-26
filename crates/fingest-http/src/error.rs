use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use fingest_catalog_core::CatalogError;
use fingest_contracts::ErrorResponse;
use fingest_identity_core::IdentityError;
use fingest_kernel::PortError;
use fingest_planning_core::PlanningError;
use fingest_wallets_core::WalletsError;

/// The single type that renders an HTTP error body.
///
/// Every context error funnels through here, so the `{"status","message"}` shape and the
/// status codes stay identical to v1 in exactly one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    BadRequest(String),
    Unauthenticated(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    /// Message plus the seconds to put in `Retry-After`.
    TooManyRequests(String, u64),
    Internal(String),
}

impl ApiError {
    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(m)
            | Self::Unauthenticated(m)
            | Self::Forbidden(m)
            | Self::NotFound(m)
            | Self::Conflict(m)
            | Self::TooManyRequests(m, _)
            | Self::Internal(m) => m,
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthenticated(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::TooManyRequests(..) => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code();
        let mut response = HttpResponse::build(status);
        if let Self::TooManyRequests(_, retry_after) = self {
            response.insert_header(("Retry-After", retry_after.to_string()));
        }
        response.json(ErrorResponse::new(status.as_u16(), self.message()))
    }
}

/// Adapter failures are logged in full but never echoed to the client.
///
/// Deviation D10: v1 returned the raw driver message in the 500 body, exposing schema and
/// connection detail (OWASP A01/A05). The status code is unchanged.
impl From<PortError> for ApiError {
    fn from(err: PortError) -> Self {
        match err {
            PortError::Conflict(message) => Self::Conflict(message),
            other => {
                tracing::error!(error = %other, "outbound port failure");
                Self::Internal("Internal server error".to_owned())
            }
        }
    }
}

impl From<CatalogError> for ApiError {
    fn from(err: CatalogError) -> Self {
        match err {
            CatalogError::NotFound(m) => Self::NotFound(m),
            CatalogError::Conflict(m) => Self::Conflict(m),
            CatalogError::Validation(m) => Self::BadRequest(m),
            CatalogError::Internal(m) => {
                tracing::error!(error = %m, "catalog internal failure");
                Self::Internal(m)
            }
            CatalogError::Port(e) => e.into(),
        }
    }
}

impl From<PlanningError> for ApiError {
    fn from(err: PlanningError) -> Self {
        match err {
            PlanningError::NotFound(m) => Self::NotFound(m),
            PlanningError::Forbidden(m) => Self::Forbidden(m),
            PlanningError::BadRequest(m) | PlanningError::Validation(m) => Self::BadRequest(m),
            PlanningError::Port(e) => e.into(),
        }
    }
}

impl From<WalletsError> for ApiError {
    fn from(err: WalletsError) -> Self {
        match err {
            WalletsError::NotFound(m) => Self::NotFound(m),
            WalletsError::Forbidden(m) => Self::Forbidden(m),
            WalletsError::BadRequest(m) | WalletsError::Validation(m) => Self::BadRequest(m),
            WalletsError::Internal(m) => {
                tracing::error!(error = %m, "wallets internal failure");
                Self::Internal(m)
            }
            WalletsError::Port(e) => e.into(),
        }
    }
}

impl From<IdentityError> for ApiError {
    fn from(err: IdentityError) -> Self {
        match err {
            IdentityError::Unauthenticated(m) => Self::Unauthenticated(m),
            IdentityError::Forbidden(m) => Self::Forbidden(m),
            IdentityError::NotFound(m) => Self::NotFound(m),
            IdentityError::BadRequest(m) | IdentityError::Validation(m) => Self::BadRequest(m),
            IdentityError::Internal(m) => {
                tracing::error!(error = %m, "identity internal failure");
                Self::Internal(m)
            }
            IdentityError::Port(e) => e.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::body::to_bytes;

    fn body_of(err: &ApiError) -> ErrorResponse {
        let response = err.error_response();
        let bytes = block_on(to_bytes(response.into_body())).expect("body is fully buffered");
        serde_json::from_slice(&bytes).expect("error body must be the v1 shape")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        actix_rt::System::new().block_on(f)
    }

    #[test]
    fn status_codes_match_v1() {
        use StatusCode as S;
        let cases = [
            (ApiError::BadRequest("x".into()), S::BAD_REQUEST),
            (ApiError::Unauthenticated("x".into()), S::UNAUTHORIZED),
            (ApiError::Forbidden("x".into()), S::FORBIDDEN),
            (ApiError::NotFound("x".into()), S::NOT_FOUND),
            (ApiError::Conflict("x".into()), S::CONFLICT),
            (
                ApiError::TooManyRequests("x".into(), 1),
                S::TOO_MANY_REQUESTS,
            ),
            (ApiError::Internal("x".into()), S::INTERNAL_SERVER_ERROR),
        ];
        for (err, expected) in cases {
            assert_eq!(err.status_code(), expected, "{err:?}");
        }
    }

    #[test]
    fn body_is_status_string_plus_message() {
        let body = body_of(&ApiError::NotFound("Category 'Food' not found".into()));
        assert_eq!(body.status, "404");
        assert_eq!(body.message, "Category 'Food' not found");
    }

    #[test]
    fn catalog_errors_map_to_the_v1_codes() {
        assert!(matches!(
            ApiError::from(CatalogError::not_found("Food", false)),
            ApiError::NotFound(_)
        ));
        assert!(matches!(
            ApiError::from(CatalogError::already_exists("Food", false)),
            ApiError::Conflict(_)
        ));
        assert!(matches!(
            ApiError::from(CatalogError::Validation("bad".into())),
            ApiError::BadRequest(_)
        ));
    }

    #[test]
    fn storage_failures_do_not_leak_driver_detail() {
        let err = ApiError::from(PortError::Storage(
            "FATAL: password authentication failed for user \"postgres\"".into(),
        ));
        assert_eq!(err, ApiError::Internal("Internal server error".into()));
        assert!(!err.message().contains("password"));
    }
}
