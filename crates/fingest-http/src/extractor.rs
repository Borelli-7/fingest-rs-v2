use std::sync::Arc;

use actix_web::{FromRequest, HttpMessage, HttpRequest, dev::Payload};
use fingest_identity_core::{Claims, TokenVerifier};

use crate::error::ApiError;

/// Shared handle to the token verifier, registered once in the composition root.
#[derive(Clone)]
pub struct TokenVerifierRef(pub Arc<dyn TokenVerifier>);

/// Pulls a bearer token out of the `Authorization` header.
pub fn bearer_token(req: &HttpRequest) -> Option<&str> {
    req.headers()
        .get("Authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Proof that the caller presented a valid token.
///
/// A handler that needs identity must ask for it, so protection cannot be lost by
/// mis-configuring a path list. Works whether or not [`crate::middleware::JwtAuth`] ran:
/// claims already in the request extensions are reused, otherwise the header is verified
/// here.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub Claims);

impl AuthenticatedUser {
    pub fn login(&self) -> &str {
        &self.0.sub
    }

    pub fn is_admin(&self) -> bool {
        self.0.admin
    }

    pub fn require_admin(&self) -> Result<(), ApiError> {
        if !self.is_admin() {
            return Err(ApiError::Forbidden(
                "Not authorized. Admin privileges required to access this resource".to_owned(),
            ));
        }
        Ok(())
    }

    /// A caller may act on their own resources; an admin may act on anyone's.
    pub fn require_self_or_admin(&self, path_login: &str) -> Result<(), ApiError> {
        if self.is_admin() || self.login() == path_login {
            return Ok(());
        }
        Err(ApiError::Forbidden(format!(
            "Not authorized. Path login '{path_login}' does not match authenticated user '{}'",
            self.login()
        )))
    }
}

impl FromRequest for AuthenticatedUser {
    type Error = ApiError;
    type Future = std::future::Ready<Result<Self, ApiError>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        std::future::ready(authenticate(req))
    }
}

fn authenticate(req: &HttpRequest) -> Result<AuthenticatedUser, ApiError> {
    if let Some(claims) = req.extensions().get::<Claims>().cloned() {
        return Ok(AuthenticatedUser(claims));
    }

    let verifier = req
        .app_data::<actix_web::web::Data<TokenVerifierRef>>()
        .ok_or_else(|| {
            // Wiring bug, not a client error: fail closed and make it loud.
            tracing::error!("TokenVerifierRef is not registered in app_data");
            ApiError::Internal("Internal server error".to_owned())
        })?;

    let token = bearer_token(req)
        .ok_or_else(|| ApiError::Unauthenticated("Missing Authorization header".to_owned()))?;

    verifier
        .0
        .verify(token)
        .map(AuthenticatedUser)
        .map_err(|e| ApiError::Unauthenticated(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::TestRequest;

    fn user(login: &str, admin: bool) -> AuthenticatedUser {
        AuthenticatedUser(Claims {
            sub: login.to_owned(),
            admin,
            exp: 0,
            iat: 0,
        })
    }

    #[test]
    fn admin_guard_allows_admins_only() {
        assert!(user("root", true).require_admin().is_ok());
        assert!(matches!(
            user("bob", false).require_admin().unwrap_err(),
            ApiError::Forbidden(_)
        ));
    }

    #[test]
    fn self_guard_allows_the_owner() {
        assert!(user("bob", false).require_self_or_admin("bob").is_ok());
    }

    #[test]
    fn self_guard_allows_an_admin_acting_on_another_account() {
        assert!(user("root", true).require_self_or_admin("bob").is_ok());
    }

    #[test]
    fn self_guard_blocks_acting_on_someone_else() {
        let err = user("bob", false)
            .require_self_or_admin("alice")
            .unwrap_err();

        assert!(matches!(err, ApiError::Forbidden(_)));
        assert!(err.message().contains("alice"));
        assert!(err.message().contains("bob"));
    }

    #[test]
    fn bearer_token_is_parsed() {
        let req = TestRequest::default()
            .insert_header(("Authorization", "Bearer abc.def.ghi"))
            .to_http_request();

        assert_eq!(bearer_token(&req), Some("abc.def.ghi"));
    }

    #[test]
    fn non_bearer_schemes_are_ignored() {
        let req = TestRequest::default()
            .insert_header(("Authorization", "Basic dXNlcjpwYXNz"))
            .to_http_request();

        assert_eq!(bearer_token(&req), None);
    }

    #[test]
    fn a_missing_header_is_unauthenticated_not_a_panic() {
        let req = TestRequest::default().to_http_request();
        assert_eq!(bearer_token(&req), None);
    }
}
