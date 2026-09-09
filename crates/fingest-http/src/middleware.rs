use std::{
    future::{Future, Ready, ready},
    pin::Pin,
    sync::Arc,
};

use actix_web::{
    Error, HttpMessage, ResponseError,
    body::EitherBody,
    dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready},
};
use fingest_identity_core::TokenVerifier;

use crate::{error::ApiError, extractor::bearer_token};

type LocalBoxFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

/// Rejects unauthenticated requests for an entire scope.
///
/// v1 defined an equivalent type but never called `.wrap()` with it, leaving every
/// `/resources/*` route open. Use this for scopes where *no* route is public; where a scope
/// mixes public and protected routes, take [`crate::AuthenticatedUser`] in the handler
/// instead.
pub struct JwtAuth {
    verifier: Arc<dyn TokenVerifier>,
}

impl JwtAuth {
    pub fn new(verifier: Arc<dyn TokenVerifier>) -> Self {
        Self { verifier }
    }
}

impl<S, B> Transform<S, ServiceRequest> for JwtAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = JwtAuthMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(JwtAuthMiddleware {
            service,
            verifier: Arc::clone(&self.verifier),
        }))
    }
}

pub struct JwtAuthMiddleware<S> {
    service: S,
    verifier: Arc<dyn TokenVerifier>,
}

impl<S, B> Service<ServiceRequest> for JwtAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let token = bearer_token(req.request()).map(str::to_owned);

        let reject = |req: ServiceRequest, err: ApiError| {
            let (request, _) = req.into_parts();
            let response = err.error_response().map_into_right_body();
            Box::pin(ready(Ok(ServiceResponse::new(request, response))))
        };

        let Some(token) = token else {
            return reject(
                req,
                ApiError::Unauthenticated("Missing Authorization header".to_owned()),
            );
        };

        match self.verifier.verify(&token) {
            Ok(claims) => {
                req.extensions_mut().insert(claims);
                let fut = self.service.call(req);
                Box::pin(async move { fut.await.map(ServiceResponse::map_into_left_body) })
            }
            Err(e) => reject(req, ApiError::Unauthenticated(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, HttpResponse, http::StatusCode, test, web};
    use fingest_identity_core::testing::FakeTokens;

    async fn protected() -> HttpResponse {
        HttpResponse::Ok().body("secret")
    }

    fn verifier() -> Arc<dyn TokenVerifier> {
        Arc::new(FakeTokens)
    }

    #[actix_web::test]
    async fn a_request_without_a_token_is_rejected() {
        let app = test::init_service(
            App::new().service(
                web::scope("/protected")
                    .wrap(JwtAuth::new(verifier()))
                    .route("", web::get().to(protected)),
            ),
        )
        .await;

        let req = test::TestRequest::get().uri("/protected").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn a_valid_token_is_admitted() {
        let app = test::init_service(
            App::new().service(
                web::scope("/protected")
                    .wrap(JwtAuth::new(verifier()))
                    .route("", web::get().to(protected)),
            ),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/protected")
            .insert_header(("Authorization", "Bearer token-for-bob-admin=false"))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn a_malformed_token_is_rejected() {
        let app = test::init_service(
            App::new().service(
                web::scope("/protected")
                    .wrap(JwtAuth::new(verifier()))
                    .route("", web::get().to(protected)),
            ),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/protected")
            .insert_header(("Authorization", "Bearer garbage"))
            .to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
