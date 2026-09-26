//! Inbound HTTP adapter.
//!
//! Owns every actix type in the system: routes, extractors, JSON error shaping and the
//! single `ResponseError` implementation. Bounded contexts stay framework-free.

pub mod auth;
pub mod budgets;
pub mod capabilities;
pub mod catalog;
pub mod error;
pub mod extractor;
pub mod json_config;
pub mod middleware;
pub mod rate_limit;
pub mod resources;
pub mod users;
pub mod wallets;

pub use auth::auth_routes;
pub use capabilities::{CapabilityReport, capability_routes};
pub use catalog::category_routes;
pub use error::ApiError;
pub use extractor::{AuthenticatedUser, TokenVerifierRef};
pub use json_config::{json_config_money, json_config_plain};
pub use middleware::JwtAuth;
pub use rate_limit::RateLimiter;
pub use resources::user_routes;

/// Registers every route. Contexts are added here as their phases land.
pub fn configure_routes(
    cfg: &mut actix_web::web::ServiceConfig,
    verifier: std::sync::Arc<dyn fingest_identity_core::TokenVerifier>,
) {
    auth_routes(cfg);
    capability_routes(cfg);
    category_routes(cfg);
    user_routes(cfg, verifier);
}
